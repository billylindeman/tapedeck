use clap::Parser;
use failure::Error;

use enclose::enc;
use log::*;
use tapedeck::engine;

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
        .gst_debug(true)
        .encode_dir(Some("/tmp".to_string()))
        .build()
        .unwrap();

    ctrlc::set_handler(enc!( (main_loop) move || {
        main_loop.quit();
    }))?;

    {
        let _eng = engine::Engine::new(cfg).unwrap();
        main_loop.run();
    }

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
