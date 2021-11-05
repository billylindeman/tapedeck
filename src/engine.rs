use failure::Error;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use gst::prelude::*;
use headless_chrome::{Browser, LaunchOptions, Tab};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use subprocess::{Exec, Popen};
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

    pub glib_ctx: glib::MainContext,
}

pub struct Engine {
    id: u32,
    ctx: glib::MainContext,
    xvfb: Popen,
    pulse: Popen,
    browser: Browser,
    tab: Arc<Tab>,
    gst_encode: gst::Pipeline,
    gst_encode_eos_rx: mpsc::Receiver<bool>,
    gst_debug: gst::Pipeline,
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> Result<Engine, Error> {
        let display: &str = &format!(":1{:0>4}", cfg.id);
        let pulse_server: &str = &format!("tcp:localhost:1{:0>4}", cfg.id);

        info!("[Engine({})] Launching Xvfb", cfg.id);
        let xvfb = launch_xvfb(display, cfg.size)?.popen()?;

        info!("[Engine({})] Launching PulseAudio", cfg.id);
        let pulse = launch_pulse(cfg.id)?.popen()?;

        info!("[Engine({})] Launching Chromium", cfg.id);
        let (browser, tab) = launch_chromium_browser(display, pulse_server, &cfg.url)?;

        info!("[Engine({})] Launching Gstreamer Debug", cfg.id);
        let gst_debug = launch_gstreamer_debug(display, pulse_server)?;

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
            xvfb: xvfb,
            pulse: pulse,
            browser: browser,
            tab: tab,
            gst_encode: gst_encode,
            gst_encode_eos_rx: encode_eos_rx,
            gst_debug: gst_debug,
        })
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), Error> {
        self.tab.navigate_to(url)?;
        self.tab.wait_until_navigated()?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), Error> {
        let rx = &mut self.gst_encode_eos_rx;
        // End of stream handler
        info!("[Engine({})] send eos", self.id);
        self.gst_encode.send_event(gst::event::Eos::new());
        self.ctx.block_on(async {
            rx.next().await;
        });

        info!("eos received on bus..gst finished");
        self.gst_debug.set_state(gst::State::Null)?;
        self.gst_encode.set_state(gst::State::Null)?;

        self.xvfb.terminate()?;
        self.xvfb.wait()?;
        info!("killed xvfb");
        self.pulse.terminate()?;
        self.pulse.wait()?;
        info!("killed pulse");

        Ok(())
    }
}

fn launch_chromium_browser(
    display: &str,
    pulse_server: &str,
    recording_url: &str,
) -> Result<(Browser, Arc<Tab>), Error> {
    let mut env = HashMap::new();
    env.insert("PULSE_SERVER".to_owned(), pulse_server.to_owned());
    env.insert("PULSE_SINK".to_owned(), "loopback".to_owned());
    env.insert("DISPLAY".to_owned(), display.to_owned());

    let mut args = Vec::new();
    args.push(OsStr::new("--enable-audio-output"));
    args.push(OsStr::new("--autoplay-policy=no-user-gesture-required"));
    args.push(OsStr::new("--kiosk"));
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

    let tab = browser.wait_for_initial_tab()?;

    // Navigate to wikipedia
    tab.navigate_to(recording_url)?;
    tab.wait_until_navigated()?;

    Ok((browser, tab))
}

fn launch_pulse(id: u32) -> Result<Exec, Error> {
    Ok(Exec::shell(format!(
        "dbus-run-session pulseaudio -n \
        -vvv \
        --system=false \
        --daemonize=false \
        --disable-shm \
        --use-pid-file=false \
        --realtime=false \
        --load=\"module-null-sink sink_name=loopback\" \
        --load=\"module-native-protocol-tcp port=1{:0>4} auth-anonymous=1\" \
        ",
        id
    )))
}

fn launch_xvfb(display: &str, size: (u32, u32)) -> Result<Exec, Error> {
    Ok(Exec::shell(format!(
        "Xvfb {} -screen 0 {}x{}x24",
        display, size.0, size.1
    )))
}

fn launch_gstreamer_debug(display: &str, pulse_server: &str) -> Result<gst::Pipeline, Error> {
    let pipeline = gst::Pipeline::new(Some("debug"));

    let ximagesrc = gst::ElementFactory::make("ximagesrc", None)?;
    ximagesrc.set_property_from_str("display-name", &display);
    ximagesrc.set_property_from_str("show-pointer", "false");

    let caps = gst::Caps::builder("video/x-raw")
        .field("framerate", gst::Fraction::new(60, 1))
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
        .field("framerate", gst::Fraction::new(60, 1))
        .build();
    let caps_filter = gst::ElementFactory::make("capsfilter", None)?;
    caps_filter.set_property("caps", &caps)?;

    let video_queue = gst::ElementFactory::make("queue", None)?;
    let video_convert = gst::ElementFactory::make("videoconvert", None)?;
    let video_enc = gst::ElementFactory::make("x264enc", None)?;
    video_enc.set_property_from_str("speed-preset", "fast");

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
