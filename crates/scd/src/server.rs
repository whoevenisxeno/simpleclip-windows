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
        Request::Save { last_secs } => match daemon.save_clip(last_secs) {
            Ok((path, duration_secs)) => Response::Saved {
                path,
                duration_secs,
            },
            Err(message) => Response::Error { message },
        },
        Request::Screenshot => match daemon.screenshot() {
            Ok(path) => Response::Saved {
                path,
                duration_secs: 0.0,
            },
            Err(message) => Response::Error { message },
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
        Request::Shutdown => {
            tracing::info!("shutdown requested over IPC");
            // Let the reply reach the client before the process goes away.
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(200));
                std::process::exit(0);
            });
            Response::Ok
        }
        Request::Thumbnail { path, width } => match crate::thumbs::cached_thumbnail(&path, width) {
            Ok(path) => Response::Thumbnail { path },
            Err(message) => Response::Error { message },
        },
        Request::Probe { path } => match media_duration(&path) {
            Ok(duration_secs) => Response::Probed { duration_secs },
            Err(message) => Response::Error { message },
        },
        Request::Trim {
            path,
            start_secs,
            end_secs,
        } => match trim(
            &path,
            start_secs,
            end_secs,
            daemon.config.read().unwrap().save.trim_directory.clone(),
        ) {
            Ok((path, duration_secs)) => Response::Saved {
                path,
                duration_secs,
            },
            Err(message) => Response::Error { message },
        },
        Request::Pause => {
            daemon.set_paused(true);
            Response::Ok
        }
        Request::Resume => {
            daemon.set_paused(false);
            Response::Ok
        }
        // Enumeration is served by the Windows capture backend; elsewhere the
        // daemon reports nothing detected rather than faking devices, and the
        // wizard handles the empty case.
        #[cfg(windows)]
        Request::ListMonitors => match sc_capture::list_monitors() {
            Ok(monitors) => Response::Monitors { monitors },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        #[cfg(windows)]
        Request::ListAudioDevices => match sc_capture::list_audio_devices() {
            Ok(devices) => Response::AudioDevices { devices },
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        #[cfg(not(windows))]
        Request::ListMonitors => Response::Monitors {
            monitors: Vec::new(),
        },
        #[cfg(not(windows))]
        Request::ListAudioDevices => Response::AudioDevices {
            devices: Vec::new(),
        },
    }
}

#[cfg(any(target_os = "linux", windows))]
fn media_duration(path: &std::path::Path) -> Result<f64, String> {
    sc_ffmpeg::probe_duration(path).map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn media_duration(_: &std::path::Path) -> Result<f64, String> {
    Err("not supported on this platform".into())
}

/// Trim `path` to `[start, end]` seconds into `<name>-trim.mp4`, in `dest_dir`
/// when one is configured, otherwise beside the original.
#[cfg(any(target_os = "linux", windows))]
fn trim(
    path: &std::path::Path,
    start: f64,
    end: f64,
    dest_dir: Option<std::path::PathBuf>,
) -> Result<(std::path::PathBuf, f64), String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("bad file name")?;
    let dir = match dest_dir {
        Some(d) => {
            std::fs::create_dir_all(&d)
                .map_err(|e| format!("can't create trim folder {}: {e}", d.display()))?;
            d
        }
        None => path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
    };
    let mut out = dir.join(format!("{stem}-trim.mp4"));
    let mut n = 2;
    while out.exists() {
        out = dir.join(format!("{stem}-trim{n}.mp4"));
        n += 1;
    }
    let secs = sc_ffmpeg::trim_copy(path, &out, start, end).map_err(|e| e.to_string())?;
    Ok((out, secs))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn trim(
    _: &std::path::Path,
    _: f64,
    _: f64,
    _: Option<std::path::PathBuf>,
) -> Result<(std::path::PathBuf, f64), String> {
    Err("not supported on this platform".into())
}
