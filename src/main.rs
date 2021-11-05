use enclose::enc;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

use rocket::State;
use tokio::runtime::Runtime;

mod engine;

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate derive_builder;

struct DB {
    ctx: glib::MainContext,
    engines: Mutex<Vec<engine::Engine>>,
}

#[get("/")]
fn index() -> &'static str {
    "hello"
}

#[get("/start")]
async fn start(db: &State<DB>) -> &'static str {
    let mut engines = db.engines.lock().await;

    if engines.len() > 0 {
        return "error: already started";
    }

    let cfg = engine::EngineConfigBuilder::default()
        .glib_ctx(db.ctx.clone())
        .id(1)
        .size((1920, 1080))
        .url("https://www.youtube.com/watch?v=JIx_ILapASY".to_owned())
        .encode_dir(Some("/tmp".to_string()))
        .build()
        .unwrap();

    info!("ENGINE STARTING");

    engines.push(engine::Engine::new(cfg).unwrap());

    info!("ENGINE STARTED");

    "started"
}

#[get("/stop")]
async fn stop(db: &State<DB>) -> &'static str {
    let mut engines = db.engines.lock().await;
    if engines.len() == 0 {}

    if let Some(mut eng) = engines.pop() {
        eng.stop().unwrap();
        return "stopped";
    }

    "error: no engines running"
}

fn web_init(ctx: glib::MainContext) {
    std::thread::spawn(|| {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            rocket::build()
                .manage(DB {
                    ctx: ctx,
                    engines: Mutex::new(Vec::new()),
                })
                .mount("/", routes![index, start, stop])
                .launch()
                .await
                .expect("error in web server");
        })
    });
}

fn main() -> Result<(), Box<dyn Error>> {
    let ctx = glib::MainContext::default();
    ctx.push_thread_default();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);

    pretty_env_logger::init();
    gst::init()?;
    web_init(ctx.clone());

    //    glib::timeout_add(
    //        30000,
    //        enc!( (main_loop) move || {
    //            eng.stop().expect("error stopping engine");
    //            main_loop.quit();
    //
    //            glib::Continue(false)
    //        }),
    //    );

    ctrlc::set_handler(enc!( (main_loop) move || {
        main_loop.quit();
    }))?;

    main_loop.run();

    Ok(())
}
