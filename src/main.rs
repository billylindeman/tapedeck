use enclose::enc;
use log::*;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

mod engine;

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate derive_builder;

#[get("/")]
fn index() -> &'static str {
    "hello"
}

fn main() -> Result<(), Box<dyn Error>> {
    let ctx = glib::MainContext::default();
    ctx.push_thread_default();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);

    pretty_env_logger::init();
    gst::init()?;

    let cfg = engine::EngineConfigBuilder::default()
        .glib_ctx(ctx.clone())
        .glib_loop(main_loop.clone())
        .id(1)
        .size((1920, 1080))
        .url("https://www.youtube.com/watch?v=JIx_ILapASY".to_owned())
        .encode_dir(Some("/tmp".to_string()))
        .build()?;

    let mut eng = engine::Engine::new(cfg)?;

    glib::timeout_add(
        20000,
        enc!( (main_loop) move || {
            eng.stop().expect("error stopping engine");
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
