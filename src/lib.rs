#[macro_use]
extern crate rocket;
#[macro_use]
extern crate derive_builder;

use futures::channel::oneshot;
use std::collections::HashMap;
use std::error::Error;

pub mod engine;

pub enum ManagerEvent {
    EngineSpawn(oneshot::Sender<Result<(), String>>, engine::EngineConfig),
    EngineStop(oneshot::Sender<Result<(), String>>, u32),
}

pub struct Manager {}

impl Manager {
    pub fn new() -> glib::Sender<ManagerEvent> {
        let mut engines = HashMap::new();

        let (tx, rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

        rx.attach(None, move |msg| {
            match msg {
                ManagerEvent::EngineSpawn(res, cfg) => {
                    let id = cfg.id;
                    let eng = engine::Engine::new(cfg).unwrap();
                    engines.insert(id, eng);
                    res.send(Ok(())).unwrap();
                }
                ManagerEvent::EngineStop(res, key) => match engines.remove(&key) {
                    None => {
                        res.send(Err(format!("error: no engine found key={}", &key)))
                            .unwrap();
                    }
                    Some(mut e) => {
                        if let Err(_) = e.stop() {
                            res.send(Err(String::from("error: couldn't stop engine")))
                                .unwrap();
                        } else {
                            res.send(Ok(())).unwrap();
                        }
                    }
                },
            };

            glib::Continue(true)
        });

        tx
    }
}
