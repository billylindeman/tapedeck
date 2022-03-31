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

enum TapedeckEvent {
    Shutdown,
}

#[get("/stop")]
async fn stop(
    mgr: &State<glib::Sender<ManagerEvent>>,
    app_tx: &State<glib::Sender<TapedeckEvent>>,
) -> String {
    let (tx, rx) = oneshot::channel();
    mgr.send(ManagerEvent::EngineStop(tx, 0)).unwrap();
    if let Err(err) = rx.await.unwrap() {
        return err;
    }

    app_tx.send(TapedeckEvent::Shutdown);
    "stopped".to_owned()
}

fn web_init(
    ctx: glib::MainContext,
    mgr_sender: glib::Sender<ManagerEvent>,
    app_sender: glib::Sender<TapedeckEvent>,
) {
    std::thread::spawn(|| {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            rocket::build()
                .manage(ctx)
                .manage(mgr_sender)
                .manage(app_sender)
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

    let (app_tx, app_rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
    web_init(ctx.clone(), manager, app_tx);

    ctrlc::set_handler(enc!( (main_loop) move || {
        main_loop.quit();
    }))?;

    app_rx.attach(
        None,
        enc!( (main_loop) move |msg| {
            match msg {
                TapedeckEvent::Shutdown => { main_loop.quit(); }
            };
            glib::Continue(true)
        }),
    );

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
