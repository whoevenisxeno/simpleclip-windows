use crate::state::Daemon;
use anyhow::Result;
use interprocess::local_socket::traits::ListenerExt;
use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};
use sc_core::capture::CaptureState;
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use std::io::BufReader;

pub fn socket_name() -> String {
    std::env::var("SC_SOCKET").unwrap_or_else(|_| ipc::DEFAULT_SOCKET_NAME.to_string())
}

pub fn serve(daemon: Daemon) -> Result<()> {
    let name = socket_name();
    let ns = name.clone().to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(ns).create_sync()?;
    tracing::info!(socket = %name, "IPC listening");

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let daemon = daemon.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, daemon) {
                        tracing::warn!(error = %e, "connection handler ended");
                    }
                });
            }
            Err(e) => tracing::warn!(error = %e, "accept failed"),
        }
    }
    Ok(())
}

fn handle(stream: Stream, daemon: Daemon) -> Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;
    loop {
        let env: Envelope<Request> = match ipc::read_message(&mut reader, IPC_VERSION)? {
            Some(e) => e,
            None => return Ok(()),
        };
        let resp = dispatch(env.payload, &daemon);
        ipc::write_message(&mut writer, &Envelope::new(env.id, resp))?;
    }
}

fn dispatch(req: Request, daemon: &Daemon) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::Status => Response::Status(daemon.status_report()),
        // Phase 1+ wires these to the real pipeline; stubbed for the Phase 0 gate.
        Request::Save { .. } | Request::Screenshot => Response::Error {
            message: "capture not implemented until Phase 1".into(),
        },
        Request::Record => {
            daemon.set_recording(true);
            daemon.set_state(CaptureState::Active);
            Response::Ok
        }
        Request::Stop => {
            daemon.set_recording(false);
            daemon.set_state(CaptureState::Stopped);
            Response::Ok
        }
        Request::Pause => {
            daemon.set_state(CaptureState::Paused);
            Response::Ok
        }
        Request::Resume => {
            daemon.set_state(CaptureState::Active);
            Response::Ok
        }
    }
}
