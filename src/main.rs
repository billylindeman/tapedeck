use std::ffi::OsStr;
use std::collections::HashMap;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetWindowAttributesReply};
use x11rb::connection::Connection;
use std::error::Error;
use std::result::Result;
use headless_chrome::{Browser, LaunchOptions, protocol::page::ScreenshotFormat};

use subprocess::{Exec, CaptureData};


fn launch_chrome_browser(display: &str) -> Result<Browser, Box<dyn Error>> {
    let mut env = HashMap::new();
    env.insert("DISPLAY".to_owned(), display.to_owned());

    let mut args = Vec::new();
    args.push(OsStr::new("--class=-record123"));

    let options = LaunchOptions::default_builder()
        .headless(false)
        .process_envs(Some(env))
        .args(args)
        .build()?;
    let browser = Browser::new(options)?;

    let tab = browser.wait_for_initial_tab()?;

    // Navigate to wikipedia
    tab.navigate_to("https://youtube.com")?;

    // Wait for network/javascript/dom to make the search-box available
    // and click it.
    // tab.wait_for_element("input#searchInput")?.click()?;

    Ok(browser)
}


fn launch_xvfb(display: &str) -> Result<Exec, Box<dyn Error>> {
    Ok(Exec::shell(format!("Xvfb {}", display)))
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

        let reply = conn
            .get_property(
                false,
                win_id,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                std::u32::MAX,
            )?;
        let title = reply.reply()?.value;

        let reply = conn
            .get_property(
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

        println!("got title: {} => class: {:?}", String::from_utf8(title)?, split);
    }


    Ok(())
}

fn main() -> Result<(), Box<dyn Error>>{

    let display: &str = ":1234";


    std::thread::spawn(move || {
        println!("launching xvfb");
        let x = launch_xvfb(display).unwrap();
        x.join();
    });

    println!("launching chrome");
    let _b = launch_chrome_browser(display)?;
    list_window_classes(display)?;


    loop {}

    Ok(())
}
