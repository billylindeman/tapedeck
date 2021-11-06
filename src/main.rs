use enclose::enc;
use futures::channel::oneshot;
use rocket::State;
use std::error::Error;
use std::io::{BufRead, BufReader};
use tokio::runtime::Runtime;

mod engine;

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate derive_builder;

pub enum ManagerEvent {
    EngineSpawn(oneshot::Sender<Result<(), String>>, engine::EngineConfig),
    EngineStop(oneshot::Sender<Result<(), String>>),
}

struct Manager {}

impl Manager {
    fn new() -> glib::Sender<ManagerEvent> {
        let mut engines = Vec::new();

        let (tx, rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

        rx.attach(None, move |msg| {
            match msg {
                ManagerEvent::EngineSpawn(res, cfg) => {
                    let eng = engine::Engine::new(cfg).unwrap();
                    engines.push(eng);
                    res.send(Ok(())).unwrap();
                }
                ManagerEvent::EngineStop(res) => {
                    if engines.len() == 0 {
                        res.send(Err(String::from("error: no engines running")))
                            .unwrap();
                    } else {
                        let mut e = engines.pop().unwrap();
                        if let Err(_) = e.stop() {
                            res.send(Err(String::from("error: couldn't stop engine")))
                                .unwrap();
                        } else {
                            res.send(Ok(())).unwrap();
                        }
                    }
                }
            };

            glib::Continue(true)
        });

        tx
    }
}

#[get("/")]
fn index() -> &'static str {
    "hello"
}

#[get("/start")]
async fn start(ctx: &State<glib::MainContext>, mgr: &State<glib::Sender<ManagerEvent>>) -> String {
    let cfg = engine::EngineConfigBuilder::default()
        .glib_ctx((*ctx).clone())
        .id(1)
        .size((1920, 1080))
        .url("https://www.youtube.com/watch?v=JIx_ILapASY".to_owned())
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

#[get("/stop")]
async fn stop(mgr: &State<glib::Sender<ManagerEvent>>) -> String {
    let (tx, rx) = oneshot::channel();
    mgr.send(ManagerEvent::EngineStop(tx)).unwrap();
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
