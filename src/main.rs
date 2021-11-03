use gst::prelude::*;
use headless_chrome::{protocol::page::ScreenshotFormat, Browser, LaunchOptions};
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsStr;
use std::result::Result;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetWindowAttributesReply};

use std::sync::mpsc::channel;

use subprocess::{CaptureData, Exec};

fn launch_chrome_browser(display: &str) -> Result<Browser, Box<dyn Error>> {
    let mut env = HashMap::new();
    env.insert("PULSE_SERVER".to_owned(), "tcp:localhost:5546".to_owned());
    env.insert("PULSE_SINK".to_owned(), "loopback".to_owned());
    env.insert("DISPLAY".to_owned(), display.to_owned());

    let mut args = Vec::new();
    args.push(OsStr::new("--enable-audio-output"));
    args.push(OsStr::new("--autoplay-policy=no-user-gesture-required"));
    args.push(OsStr::new("--disable-infobars"));
    args.push(OsStr::new("--kiosk"));
    args.push(OsStr::new("--disable-automation"));
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
    tab.navigate_to("https://www.youtube.com/watch?v=XhRIqzUDqAM")?;

    // Wait for network/javascript/dom to make the search-box available
    // and click it.
    // tab.wait_for_element("input#searchInput")?.click()?;

    Ok(browser)
}

fn launch_pulse() -> Result<Exec, Box<dyn Error>> {
    Ok(Exec::shell(format!(
        "dbus-run-session pulseaudio -n -F config/pulse.pa -vvv --disable-shm --use-pid-file=false --realtime=false"
    )))
}

fn launch_xvfb(display: &str, size: (u32, u32)) -> Result<Exec, Box<dyn Error>> {
    Ok(Exec::shell(format!(
        "Xvfb {} -screen 0 {}x{}x24",
        display, size.0, size.1
    )))
}

fn launch_gstreamer(display: &str, pulse_server: &str) -> Result<gst::Pipeline, Box<dyn Error>> {
    let pipeline = gst::Pipeline::new(Some("debug"));

    let ximagesrc = gst::ElementFactory::make("ximagesrc", None)?;
    ximagesrc.set_property_from_str("display-name", &display);

    let video_queue = gst::ElementFactory::make("queue", None)?;
    let glimagesink = gst::ElementFactory::make("glimagesink", None)?;

    let pulsesrc = gst::ElementFactory::make("pulsesrc", None)?;
    pulsesrc.set_property_from_str("server", &pulse_server);

    let audio_queue = gst::ElementFactory::make("queue", None)?;
    let autoaudiosink = gst::ElementFactory::make("autoaudiosink", None)?;

    pipeline.add_many(&[
        &ximagesrc,
        &video_queue,
        &glimagesink,
        &pulsesrc,
        &audio_queue,
        &autoaudiosink,
    ])?;

    gst::Element::link_many(&[&ximagesrc, &video_queue, &glimagesink])?;
    gst::Element::link_many(&[&pulsesrc, &audio_queue, &autoaudiosink])?;

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

fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();
    gst::init()?;

    let display: &str = ":1234";
    let pulse_server: &str = "tcp:localhost:5546";

    let (wait_xvfb_tx, wait_xvfb_rx) = channel();
    std::thread::spawn(move || {
        println!("launching xvfb");
        let x = launch_xvfb(display, (1920, 1080)).unwrap();

        let mut pipe = x.popen().expect("error launching xvfb");
        wait_xvfb_tx.send(true).unwrap();
        pipe.wait().unwrap();
    });

    let (wait_pulse_tx, wait_pulse_rx) = channel();
    std::thread::spawn(move || {
        println!("launching pulseaudio");
        let x = launch_pulse().unwrap();

        let mut pipe = x.popen().expect("error launching xvfb");
        wait_pulse_tx.send(true).unwrap();
        pipe.wait().unwrap();
    });

    wait_xvfb_rx.recv().unwrap();
    println!("xvfb launched");
    wait_pulse_rx.recv().unwrap();
    println!("pulse launched");

    println!("launching chrome");
    let _b = launch_chrome_browser(display)?;
    list_window_classes(display)?;

    println!("launching gstreamer");
    let _pipe = launch_gstreamer(display, pulse_server)?;

    loop {}

    Ok(())
}
