use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetWindowAttributesReply};
use x11rb::connection::Connection;
use std::error::Error;
use std::result::Result;

fn list_window_classes() -> Result<(), Box<dyn Error>> {
    let (conn, screen_num) = x11rb::connect(Some(":99"))?;

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


        println!("got title: {} => class: {}", String::from_utf8(title)?, String::from_utf8(class)?);
    }


    Ok(())
}

fn main() -> Result<(), Box<dyn Error>>{
    list_window_classes()?;


    Ok(())
}
