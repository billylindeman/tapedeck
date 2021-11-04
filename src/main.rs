use enclose::enc;
use futures::channel::mpsc::{self, channel};
use futures::prelude::*;
use gst::prelude::*;
use headless_chrome::{Browser, LaunchOptions, Tab};
use log::*;
use std::collections::HashMap;
use std::env::current_dir;
use std::error::Error;
use std::ffi::OsStr;
use std::io::stdin;
use std::result::Result;
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetWindowAttributesReply};

use subprocess::{CaptureData, Exec};
//use duct::{cmd, Expression, Handle};

fn launch_chrome_browser(
    display: &str,
    recording_url: &str,
) -> Result<(Browser, Arc<Tab>), Box<dyn Error>> {
    let mut env = HashMap::new();
    env.insert("PULSE_SERVER".to_owned(), "tcp:localhost:5546".to_owned());
    env.insert("PULSE_SINK".to_owned(), "loopback".to_owned());
    env.insert("DISPLAY".to_owned(), display.to_owned());

    let mut args = Vec::new();
    args.push(OsStr::new("--enable-audio-output"));
    args.push(OsStr::new("--autoplay-policy=no-user-gesture-required"));
    args.push(OsStr::new("--kiosk"));
    args.push(OsStr::new("--start-fullscreen"));
    println!("ENV: {:?}", env);

    let options = LaunchOptions::default_builder()
        .headless(false)
        .window_size(Some((1920, 1080)))
        .sandbox(false)
        .process_envs(Some(env))
        .args(args)
        .build()?;

    let browser = Browser::new(options)?;

    let tab = browser.wait_for_initial_tab()?;

    // Navigate to wikipedia
    tab.navigate_to(recording_url)?;

    // Wait for network/javascript/dom to make the search-box available
    // and click it.
    // tab.wait_for_element("input#searchInput")?.click()?;

    Ok((browser, tab))
}

fn launch_pulse() -> Result<Exec, Box<dyn Error>> {
    //Ok(cmd!(
    //    "dbus-run-session",
    //    "pulseaudio",
    //    "-n",
    //    "-F",
    //    "config/pulse.pa",
    //    "-vvv",
    //    "--disable-shm",
    //    "--use-pid-file=false",
    //    "--realtime=false"
    //)
    //.dir(current_dir().unwrap()))

    Ok(Exec::shell(format!(
        "dbus-run-session pulseaudio -n -F config/pulse.pa -vvv --disable-shm --use-pid-file=false --realtime=false"
    )))
}

fn launch_xvfb(display: &str, size: (u32, u32)) -> Result<Exec, Box<dyn Error>> {
    //Ok(cmd!(
    //    "Xvfb",
    //    display,
    //    "-screen",
    //    format!("{}", 0),
    //    format!("{}x{}x24", size.0, size.1)
    //)
    //.dir(current_dir().unwrap()))

    Ok(Exec::shell(format!(
        "Xvfb {} -screen 0 {}x{}x24",
        display, size.0, size.1
    )))
}

fn launch_gstreamer_debug(
    display: &str,
    pulse_server: &str,
) -> Result<gst::Pipeline, Box<dyn Error>> {
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

fn launch_gstreamer_record(
    display: &str,
    pulse_server: &str,
) -> Result<gst::Pipeline, Box<dyn Error>> {
    let pipeline = gst::Pipeline::new(Some("record"));

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
    audio_enc.set_property_from_str("bitratet", "128000");

    let filesink = gst::ElementFactory::make("filesink", None)?;
    filesink.set_property_from_str("location", "recording.mp4");
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

fn list_window_classes(display: &str) -> Result<(), Box<dyn Error>> {
    let (conn, screen_num) = x11rb::connect(Some(display))?;

    let screen = &conn.setup().roots[screen_num];

    println!("connected to screen");

    // Get the already existing top-level windows.
    let tree_reply = conn.query_tree(screen.root)?.reply()?;

    println!("got tree reply");

    // Iterate windows and find the chrome-browser
    for win_id in tree_reply.children {
        println!("got window: {}", win_id);

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

        println!(
            "got title: {} => class: {:?}",
            String::from_utf8(title)?,
            split
        );
    }

    Ok(())
}

async fn message_handler(loop_: glib::MainLoop, bus: gst::Bus, mut tx: mpsc::Sender<bool>) {
    let mut messages = bus.stream();

    while let Some(msg) = messages.next().await {
        use gst::MessageView;

        // Determine whether we want to quit: on EOS or error message
        // we quit, otherwise simply continue.
        match msg.view() {
            MessageView::Eos(..) => {
                tx.start_send(true);
            }
            MessageView::Error(err) => {
                println!(
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

fn main() -> Result<(), Box<dyn Error>> {
    //    print!("Enter the recording url\n:> ");
    //let mut recording_url = String::new();
    //stdin().read_line(&mut recording_url)?;
    //
    let recording_url = "https://dogfood.tandem.chat/web/recording?call=%7B%22host%22%3A%22wss%3A%2F%2Fsfu.dogfood.tandem.chat%22%2C%22id%22%3A%22ce533b2c-0d0b-4f39-add6-adcd85afeef6%22%2C%22ionToken%22%3A%22eyJhbGciOiJIUzUxMiIsInR5cCI6IkpXVCJ9.eyJyaWQiOiJjZTUzM2IyYy0wZDBiLTRmMzktYWRkNi1hZGNkODVhZmVlZjYiLCJzaWQiOiJjZTUzM2IyYy0wZDBiLTRmMzktYWRkNi1hZGNkODVhZmVlZjYifQ.xLvXI-iCSz7buyssqnQoOD3QdDa-Mqw32bMa8P9aM-4g-aQI41Nq_xmO1_-RMS8CZC6vsKwGh-6fzu2Lip9ehw%22%2C%22private%22%3Afalse%2C%22props%22%3A%7B%22id%22%3A%227c780b71-6c26-4cbd-a037-40d5540a62bd%22%2C%22quick%22%3Afalse%2C%22title%22%3A%22Standup%20in%20Tandem%20%F0%9F%9A%A5%22%2C%22type%22%3A%22meeting%22%7D%2C%22service%22%3A%22ion-sfu%22%2C%22teamId%22%3A%2273b01c4f-6ebf-437b-994f-0377e3101c7a%22%7D";

    let ctx = glib::MainContext::default();
    ctx.push_thread_default();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);

    pretty_env_logger::init();
    gst::init()?;

    let display: &str = ":1234";
    let pulse_server: &str = "tcp:localhost:5546";

    println!("launching xvfb");
    let x = launch_xvfb(display, (1920, 1080)).unwrap();

    let mut xvfb_handle = x.popen().expect("error launching xvfb");
    println!("launching pulseaudio");
    let x = launch_pulse().unwrap();

    //#[cfg(debug_assertions)]

    let mut pulse_handle = x.popen().expect("error launching pulseaudio");
    println!("launching chrome");
    let (browser_handle, tab) = launch_chrome_browser(display, recording_url)?;

    launch_gstreamer_debug(display, pulse_server)?;

    //list_window_classes(display)?;
    //println!("launching gstreamer");
    let recorder = launch_gstreamer_record(display, pulse_server)?;
    let (recorder_tx, mut recorder_rx) = channel::<bool>(0);

    let bus = recorder.bus().unwrap();
    ctx.spawn_local(message_handler(main_loop.clone(), bus, recorder_tx));

    glib::timeout_add(
        900000,
        enc!( (main_loop) move || {
            println!("stopping gstreamer: sending gst::event::Eos");
            recorder.send_event(gst::event::Eos::new());

            ctx.block_on(async { recorder_rx.next().await });
            println!("gstreamer pipeline flushed");
            xvfb_handle.kill().unwrap();
            println!("xvfb killed");
            pulse_handle.kill().unwrap();
            println!("pulse killed");

            main_loop.quit();

            glib::Continue(false)
        }),
    );

    //    ctrlc::set_handler(move || {
    //        cleanup();
    //    })?;

    main_loop.run();

    Ok(())
}
