use enclose::enc;
use futures::channel::oneshot;
use rocket::State;
use std::collections::HashMap;
use std::error::Error;
use tokio::runtime::Runtime;

mod engine;

#[get("/")]
fn index() -> &'static str {
    "hello"
}

#[post("/start/<id>")]
async fn start(
    ctx: &State<glib::MainContext>,
    mgr: &State<glib::Sender<ManagerEvent>>,
    id: u32,
) -> String {
    let cfg = engine::EngineConfigBuilder::default()
        .glib_ctx((*ctx).clone())
        .id(id)
        .size((1920, 1080))
        .url("https://www.youtube.com/watch?v=JIx_ILapASY".to_owned())
        .gst_debug(false)
        .encode_dir(Some("/tmp".to_string()))
        .build()
        .unwrap();

    let (tx, rx) = oneshot::channel();

    mgr.send(ManagerEvent::EngineSpawn(tx, cfg)).unwrap();

    if let Err(err) = rx.await.unwrap() {
        return err;
    }

    "started".to_owned()
}

#[get("/stop/<id>")]
async fn stop(mgr: &State<glib::Sender<ManagerEvent>>, id: u32) -> String {
    let (tx, rx) = oneshot::channel();
    mgr.send(ManagerEvent::EngineStop(tx, id)).unwrap();
    if let Err(err) = rx.await.unwrap() {
        return err;
    }

    "stopped".to_owned()
}

fn web_init(ctx: glib::MainContext, sender: glib::Sender<ManagerEvent>) {
    std::thread::spawn(|| {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            rocket::build()
                .manage(ctx)
                .manage(sender)
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

    let manager = Manager::new();
    web_init(ctx.clone(), manager);

    ctrlc::set_handler(enc!( (main_loop) move || {
        main_loop.quit();
    }))?;

    main_loop.run();

    Ok(())
}
