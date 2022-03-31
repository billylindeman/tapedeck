use clap::Parser;
use failure::Error;

use enclose::enc;
use futures::channel::oneshot;
use rocket::State;
use tapedeck::engine;
use tapedeck::*;
use tokio::runtime::Runtime;

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate derive_builder;

#[derive(Parser, PartialEq, Debug)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    //#[clap(short, long)]
    //force: bool,
    //#[clap(short, long, parse(from_occurrences))]
    //verbose: u64,
    #[clap(subcommand)]
    cmd: Sub,
}

#[derive(Parser, PartialEq, Debug)]
enum Sub {
    Record { url: String },
    Transcode {},
}

#[get("/stop")]
async fn stop(mgr: &State<glib::Sender<ManagerEvent>>) -> String {
    let (tx, rx) = oneshot::channel();
    mgr.send(ManagerEvent::EngineStop(tx, 0)).unwrap();
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
                .mount("/", routes![stop])
                .launch()
                .await
                .expect("error in web server");
        })
    });
}

pub fn run_record(url: String) -> Result<(), Error> {
    let ctx = glib::MainContext::default();
    ctx.push_thread_default();
    let main_loop = glib::MainLoop::new(Some(&ctx), false);

    pretty_env_logger::init();
    gst::init()?;

    info!("creating engine");

    let cfg = engine::EngineConfigBuilder::default()
        .glib_ctx(ctx.clone())
        .id(0)
        .size((1920, 1080))
        .url(url)
        .gst_debug(false)
        .encode_dir(Some("/tmp".to_string()))
        .build()
        .unwrap();

    let manager = Manager::new();

    let (tx, rx) = oneshot::channel();
    manager.send(ManagerEvent::EngineSpawn(tx, cfg)).unwrap();

    web_init(ctx.clone(), manager);

    ctrlc::set_handler(enc!( (main_loop) move || {
        main_loop.quit();
    }))?;

    main_loop.run();
    Ok(())
}

pub fn main() -> Result<(), Error> {
    let args = Cli::parse();

    match args.cmd {
        Sub::Record { url } => {
            run_record(url);
        }
        Sub::Transcode {} => {
            println!("Not Implemented");
        }
    };

    Ok(())
}
