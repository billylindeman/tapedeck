use failure::{format_err, Error};
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use gst::prelude::*;
use headless_chrome::{Browser, LaunchOptions, Tab};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use subprocess::{Exec, Popen, Redirection};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetWindowAttributesReply};

pub enum EngineEvent {}

#[derive(Builder, Debug, PartialEq)]
pub struct EngineConfig {
    #[builder(default = "1")]
    pub id: u32,

    #[builder(default = "(1920,1080)")]
    pub size: (u32, u32),

    #[builder(default = "\"https://tandem.chat\".to_string()")]
    pub url: String,

    #[builder(default = "false")]
    pub encode_enabled: bool,

    #[builder(default = "None")]
    pub encode_dir: Option<String>,

    #[builder(default = "None")]
    pub encode_rtmp: Option<String>,

    #[builder(default = "false")]
    pub gst_debug: bool,

    pub glib_ctx: glib::MainContext,
}

pub struct Engine {
    id: u32,
    ctx: glib::MainContext,
    dbus: Popen,
    xvfb: Popen,
    pulse: Popen,
    browser: Option<Browser>,
    gst_encode: gst::Pipeline,
    gst_encode_eos_rx: mpsc::Receiver<bool>,
    gst_debug: Option<gst::Pipeline>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> Result<Engine, Error> {
        let display: &str = &format!(":1{:0>4}", cfg.id);
        let pulse_server: &str = &format!("tcp:localhost:1{:0>4}", cfg.id);

        info!("[Engine({})] Launching dbus-daemon", cfg.id);
        let (dbus, dbus_session) = launch_dbus()?;
        info!("[Engine({})] using dbus_session {:?}", cfg.id, dbus_session);

        info!("[Engine({})] Launching Xvfb", cfg.id);
        let xvfb = launch_xvfb(&dbus_session, display, cfg.size)?;

        info!("[Engine({})] Launching PulseAudio", cfg.id);
        let pulse = launch_pulse(&dbus_session, cfg.id)?;

        info!("[Engine({})] Launching Chromium", cfg.id);
        let (browser, tab) =
            launch_chromium_browser(display, pulse_server, &cfg.url, &dbus_session)?;

        info!("[Engine({})] Launching Gstreamer Debug", cfg.id);
        let gst_debug = match cfg.gst_debug {
            true => Some(launch_gstreamer_debug(display, pulse_server)?),
            false => None,
        };

        info!("[Engine({})] Launching Gstreamer Encoder", cfg.id);
        let filepath = format!("{}/recording-{}.mp4", cfg.encode_dir.unwrap(), cfg.id);
        let gst_encode =
            launch_gstreamer_encode(display, pulse_server, Some(filepath), cfg.encode_rtmp)?;
        let (encode_eos_tx, encode_eos_rx) = mpsc::channel::<bool>(1);

        let encode_bus = gst_encode.bus().unwrap();
        cfg.glib_ctx
            .spawn(message_handler(encode_bus, encode_eos_tx));

        Ok(Engine {
            id: cfg.id,
            ctx: cfg.glib_ctx,
            dbus: dbus,
            xvfb: xvfb,
            pulse: pulse,
            browser: Some(browser),
            //        tab: tab,
            gst_encode: gst_encode,
            gst_encode_eos_rx: encode_eos_rx,
            gst_debug: gst_debug,
        })
    }

    //pub fn navigate(&mut self, url: &str) -> Result<(), Error> {
    //    self.tab.navigate_to(url)?;
    //    self.tab.wait_until_navigated()?;
    //    Ok(())
    //}

    pub fn stop(&mut self) -> Result<(), Error> {
        let rx = &mut self.gst_encode_eos_rx;
        // End of stream handler
        info!("[Engine({})] send eos", self.id);
        self.gst_encode.send_event(gst::event::Eos::new());
        self.ctx.block_on(async {
            rx.next().await;
        });

        info!("eos received on bus..gst finished");
        if let Some(gst_debug) = &self.gst_debug {
            gst_debug.set_state(gst::State::Null)?;
        }
        self.gst_encode.set_state(gst::State::Null)?;

        let _ = self.browser.take();

        self.xvfb.terminate()?;
        self.xvfb.wait()?;
        info!("killed xvfb");

        self.pulse.terminate()?;
        self.pulse.wait()?;
        info!("killed pulse");

        self.dbus.kill()?;
        self.dbus.wait()?;
        info!("killed dbus-daemon");

        Ok(())
    }
}

fn launch_chromium_browser(
    display: &str,
    pulse_server: &str,
    recording_url: &str,
    dbus_session: &str,
) -> Result<(Browser, Arc<Tab>), Error> {
    let mut env = HashMap::new();
    env.insert("PULSE_SERVER".to_owned(), pulse_server.to_owned());
    env.insert("PULSE_SINK".to_owned(), "loopback".to_owned());
    env.insert("DISPLAY".to_owned(), display.to_owned());
    env.insert(
        "DBUS_SESSION_BUS_ADDRESS".to_owned(),
        dbus_session.to_owned(),
    );

    env.insert("DBUS_SESSION_BUS_PID".to_owned(), "".to_owned());
    env.insert("DBUS_SESSION_BUS_WINDOWID".to_owned(), "".to_owned());
    env.insert("DBUS_STARTER_ADDRESS".to_owned(), "".to_owned());
    env.insert("DBUS_STARTER_BUS_TYPE".to_owned(), "".to_owned());

    let mut args = Vec::new();
    args.push(OsStr::new("--enable-audio-output"));
    args.push(OsStr::new("--autoplay-policy=no-user-gesture-required"));
    args.push(OsStr::new("--disable-features=ChromeWhatsNewUI"));
    args.push(OsStr::new("--kiosk"));
    args.push(OsStr::new("--disable-dev-shm-usage"));
    args.push(OsStr::new("--disable-gpu"));
    args.push(OsStr::new("--disable-fre"));
    args.push(OsStr::new("--no-default-browser-check"));
    args.push(OsStr::new("--no-first-run"));
    args.push(OsStr::new("--use-gl=swiftshader"));
    args.push(OsStr::new("--disable-setuid-sandbox"));
    args.push(OsStr::new("--remote-debugging-address=0.0.0.0"));
    args.push(OsStr::new("--remote-debugging-port=9222"));
    args.push(OsStr::new("--no-sandbox"));
    args.push(OsStr::new("--enable-logging"));
    args.push(OsStr::new("--start-fullscreen"));
    info!("ENV: {:?}", env);

    let options = LaunchOptions::default_builder()
        .headless(false)
        .window_size(Some((1920, 1080)))
        .sandbox(false)
        .idle_browser_timeout(Duration::from_secs(600))
        .process_envs(Some(env))
        .args(args)
        .build()
        .unwrap();

    let browser = Browser::new(options)?;

    let _tab = browser.wait_for_initial_tab()?;

    let tab = browser.wait_for_initial_tab()?;

    // Navigate to wikipedia
    tab.navigate_to(recording_url)?;
    tab.wait_until_navigated()?;

    Ok((browser, tab))
}

fn launch_pulse(dbus_session: &str, id: u32) -> Result<Popen, Error> {
    let pulse = Exec::cmd("pulseaudio")
        .arg("-n")
        .arg("-vvvv")
        .arg("--system=false")
        .arg("--daemonize=false")
        .arg("--disable-shm")
        .arg("--use-pid-file=false")
        .arg("--realtime=false")
        .arg("--load=module-null-sink sink_name=loopback")
        .arg(format!(
            "--load=module-native-protocol-tcp port=1{:0>4} auth-anonymous=1",
            id
        ))
        .env("DBUS_SESSION_BUS_ADDRESS", OsStr::new(dbus_session))
        .env("DBUS_SESSION_BUS_PID", OsStr::new(""))
        .env("DBUS_SESSION_BUS_WINDOWID", OsStr::new(""))
        .env("DBUS_STARTER_ADDRESS", OsStr::new(""))
        .env("DBUS_STARTER_BUS_TYPE", OsStr::new(""))
        .popen()?;

    Ok(pulse)
}

pub fn launch_dbus() -> Result<(Popen, String), Error> {
    let mut dbus = Exec::cmd("dbus-daemon")
        .arg("--nofork")
        .arg("--print-address")
        .arg("--session")
        .stdout(Redirection::Pipe)
        .popen()?;

    let mut dbus_stdout = String::new();
    if let Some(ref mut stream) = dbus.stdout {
        let mut buf = BufReader::new(stream);
        buf.read_line(&mut dbus_stdout)?;
    }

    let dbus_session = dbus_stdout
        .lines()
        .next()
        .ok_or(format_err!("couldn't extract dbus session from stdout"))?;

    Ok((dbus, dbus_session.to_owned()))
}

fn launch_xvfb(dbus_session: &str, display: &str, size: (u32, u32)) -> Result<Popen, Error> {
    let xvfb = Exec::cmd("Xvfb")
        .arg(display)
        .arg("-screen")
        .arg("0")
        .arg(format!("{}x{}x24", size.0, size.1))
        .env("DBUS_SESSION_BUS_ADDRESS", OsStr::new(dbus_session))
        .env("DBUS_SESSION_BUS_PID", OsStr::new(""))
        .env("DBUS_SESSION_BUS_WINDOWID", OsStr::new(""))
        .env("DBUS_STARTER_ADDRESS", OsStr::new(""))
        .env("DBUS_STARTER_BUS_TYPE", OsStr::new(""))
        .popen()?;

    Ok(xvfb)
}

fn launch_gstreamer_debug(display: &str, pulse_server: &str) -> Result<gst::Pipeline, Error> {
    let pipeline = gst::Pipeline::new(Some("debug"));

    let ximagesrc = gst::ElementFactory::make("ximagesrc", None)?;
    ximagesrc.set_property_from_str("display-name", &display);
    ximagesrc.set_property_from_str("show-pointer", "false");

    let caps = gst::Caps::builder("video/x-raw")
        .field("framerate", gst::Fraction::new(30, 1))
        .build();
    let caps_filter = gst::ElementFactory::make("capsfilter", None)?;
    caps_filter.set_property("caps", &caps)?;

    let video_queue = gst::ElementFactory::make("queue", None)?;
    let glimagesink = gst::ElementFactory::make("glimagesink", None)?;
    glimagesink.set_property_from_str("sync", "false");

    let pulsesrc = gst::ElementFactory::make("pulsesrc", None)?;
    pulsesrc.set_property_from_str("server", &pulse_server);

    let audio_queue = gst::ElementFactory::make("queue", None)?;
    let autoaudiosink = gst::ElementFactory::make("autoaudiosink", None)?;

    autoaudiosink.set_property_from_str("sync", "false");

    pipeline.add_many(&[
        &ximagesrc,
        &caps_filter,
        &video_queue,
        &glimagesink,
        &pulsesrc,
        &audio_queue,
        &autoaudiosink,
    ])?;

    gst::Element::link_many(&[&ximagesrc, &caps_filter, &video_queue, &glimagesink])?;
    gst::Element::link_many(&[&pulsesrc, &audio_queue, &autoaudiosink])?;

    pipeline.set_state(gst::State::Playing)?;

    Ok(pipeline)
}

fn launch_gstreamer_encode(
    display: &str,
    pulse_server: &str,
    file: Option<String>,
    rtmp: Option<String>,
) -> Result<gst::Pipeline, Error> {
    let pipeline = gst::Pipeline::new(None);

    let ximagesrc = gst::ElementFactory::make("ximagesrc", None)?;
    ximagesrc.set_property_from_str("display-name", &display);
    ximagesrc.set_property_from_str("show-pointer", "false");
    ximagesrc.set_property_from_str("do-timestamp", "true");
    ximagesrc.set_property_from_str("use-damage", "false");

    let caps = gst::Caps::builder("video/x-raw")
        .field("framerate", gst::Fraction::new(30, 1))
        .build();
    let caps_filter = gst::ElementFactory::make("capsfilter", None)?;
    caps_filter.set_property("caps", &caps)?;

    let video_queue = gst::ElementFactory::make("queue", None)?;
    let video_convert = gst::ElementFactory::make("videoconvert", None)?;
    let video_enc = gst::ElementFactory::make("x264enc", None)?;
    video_enc.set_property_from_str("speed-preset", "ultrafast");

    let mux = gst::ElementFactory::make("mp4mux", None)?;

    let pulsesrc = gst::ElementFactory::make("pulsesrc", None)?;
    pulsesrc.set_property_from_str("server", &pulse_server);
    pulsesrc.set_property_from_str("do-timestamp", "true");

    let audio_queue = gst::ElementFactory::make("queue", None)?;
    audio_queue.set_property_from_str("max-size-bytes", "0");
    audio_queue.set_property_from_str("max-size-buffers", "0");
    audio_queue.set_property_from_str("max-size-time", "0");

    let audio_enc = gst::ElementFactory::make("opusenc", None)?;
    audio_enc.set_property_from_str("bitrate", "128000");

    let filesink = gst::ElementFactory::make("filesink", None)?;
    filesink.set_property_from_str("location", &file.unwrap());
    filesink.set_property_from_str("sync", "false");

    pipeline.add_many(&[
        &ximagesrc,
        &caps_filter,
        &video_queue,
        &video_convert,
        &video_enc,
        &pulsesrc,
        &audio_queue,
        &audio_enc,
        &mux,
        &filesink,
    ])?;

    gst::Element::link_many(&[
        &ximagesrc,
        &caps_filter,
        &video_queue,
        &video_convert,
        &video_enc,
        &mux,
    ])?;
    gst::Element::link_many(&[&pulsesrc, &audio_queue, &audio_enc, &mux])?;
    gst::Element::link_many(&[&mux, &filesink])?;

    pipeline.set_state(gst::State::Playing)?;

    Ok(pipeline)
}

fn x11_list_window_classes(display: &str) -> Result<(), Error> {
    let (conn, screen_num) = x11rb::connect(Some(display))?;

    let screen = &conn.setup().roots[screen_num];

    info!("connected to screen");

    // Get the already existing top-level windows.
    let tree_reply = conn.query_tree(screen.root)?.reply()?;

    info!("got tree reply");

    // Iterate windows and find the chrome-browser
    for win_id in tree_reply.children {
        info!("got window: {}", win_id);

        let reply = conn.get_property(
            false,
            win_id,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            0,
            std::u32::MAX,
        )?;
        let title = reply.reply()?.value;

        let reply = conn.get_property(
            false,
            win_id,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            0,
            std::u32::MAX,
        )?;
        let class = reply.reply()?.value;
        let class = String::from_utf8(class)?;
        let split: Vec<_> = class.split('-').collect();

        info!(
            "got title: {} => class: {:?}",
            String::from_utf8(title)?,
            split
        );
    }

    Ok(())
}

async fn message_handler(bus: gst::Bus, mut tx: mpsc::Sender<bool>) {
    let mut messages = bus.stream();

    while let Some(msg) = messages.next().await {
        use gst::MessageView;

        // Determine whether we want to quit: on EOS or error message
        // we quit, otherwise simply continue.
        match msg.view() {
            MessageView::Eos(..) => {
                tx.start_send(true).unwrap();
                return;
            }
            MessageView::Error(err) => {
                info!(
                    "Error from {:?}: {} ({:?})",
                    err.src().map(|s| s.path_string()),
                    err.error(),
                    err.debug()
                );
            }
            _ => (),
        }
    }
}
