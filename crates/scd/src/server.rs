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
        Request::GrabRegion { x0, y0, x1, y1 } => match daemon.grab_region([x0, y0, x1, y1]) {
            Ok((width, height, gray, tint)) => Response::Region {
                width,
                height,
                gray: sc_core::b64::encode(&gray),
                tint: Some(sc_core::b64::encode(&tint)),
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
        Request::Popup { title, line } => {
            #[cfg(windows)]
            {
                crate::popup::configure(&daemon.config.read().unwrap().general);
                crate::popup::show(crate::popup::Popup::simple(
                    crate::popup::Kind::Info,
                    &title,
                    &line,
                ));
            }
            #[cfg(not(windows))]
            let _ = (title, line);
            Response::Ok
        }
        Request::TestNotification => {
            #[cfg(windows)]
            crate::notify::test(&daemon.config.read().unwrap());
            Response::Ok
        }
        Request::Shutdown => {
            tracing::info!("shutdown requested over IPC");
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
        Request::Frame { path, secs, width } => {
            match crate::thumbs::cached_frame(&path, secs, width) {
                Ok(path) => Response::Thumbnail { path },
                Err(message) => Response::Error { message },
            }
        }
        Request::Trim {
            path,
            start_secs,
            end_secs,
            dest,
        } => match trim(
            &path,
            start_secs,
            end_secs,
            dest,
            daemon.config.read().unwrap().save.trim_directory.clone(),
        ) {
            Ok((path, duration_secs)) => Response::Saved {
                path,
                duration_secs,
            },
            Err(message) => Response::Error { message },
        },
        Request::Compress {
            path,
            target_mb,
            max_height,
            max_fps,
            mute,
        } => match compress(
            &path,
            &sc_core::compress::CompressOptions {
                target_mb,
                max_height,
                max_fps,
                mute,
            },
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

#[cfg(any(target_os = "linux", windows))]
fn trim(
    path: &std::path::Path,
    start: f64,
    end: f64,
    dest: sc_core::ipc::TrimDest,
    trim_dir: Option<std::path::PathBuf>,
) -> Result<(std::path::PathBuf, f64), String> {
    use sc_core::ipc::TrimDest;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("bad file name")?;
    let beside = || path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dir = match dest {
        TrimDest::Beside => beside(),
        TrimDest::Temp => preview_dir()?,
        TrimDest::Default => match trim_dir {
            Some(d) => {
                std::fs::create_dir_all(&d)
                    .map_err(|e| format!("can't create trim folder {}: {e}", d.display()))?;
                d
            }
            None => beside(),
        },
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

#[cfg(any(target_os = "linux", windows))]
fn preview_dir() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("SimpleClip-previews");
    std::fs::create_dir_all(&dir).map_err(|e| format!("can't create preview folder: {e}"))?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let day = std::time::Duration::from_secs(24 * 3600);
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > day);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Ok(dir)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn trim(
    _: &std::path::Path,
    _: f64,
    _: f64,
    _: sc_core::ipc::TrimDest,
    _: Option<std::path::PathBuf>,
) -> Result<(std::path::PathBuf, f64), String> {
    Err("not supported on this platform".into())
}

#[cfg(any(target_os = "linux", windows))]
fn compress(
    path: &std::path::Path,
    opts: &sc_core::compress::CompressOptions,
) -> Result<(std::path::PathBuf, f64), String> {
    opts.validate()?;
    let target_mb = opts.target_mb;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("bad file name")?;
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut out = dir.join(format!("{stem}-{target_mb}mb.mp4"));
    let mut n = 2;
    while out.exists() {
        out = dir.join(format!("{stem}-{target_mb}mb-{n}.mp4"));
        n += 1;
    }
    sc_ffmpeg::compress(path, &out, opts).map_err(|e| e.to_string())?;
    let secs = media_duration(&out).unwrap_or(0.0);
    Ok((out, secs))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn compress(
    _: &std::path::Path,
    _: &sc_core::compress::CompressOptions,
) -> Result<(std::path::PathBuf, f64), String> {
    Err("not supported on this platform".into())
}
