// Linux portal + PipeWire capture backend (Phase 1). No-op on other targets;
// the Windows WGC backend is Phase 4.
#![cfg(target_os = "linux")]

mod portal;
mod stream;

use sc_core::capture::VideoFrame;
use std::thread::JoinHandle;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("portal: {0}")]
    Portal(String),
    #[error("pipewire: {0}")]
    Pipewire(String),
    #[error("capture ended before a format was negotiated")]
    NoFormat,
}

/// A running screen capture. Frames are delivered to the channel passed to
/// `start`; dropping the handle (or calling `stop`) tears the PipeWire loop down
/// cleanly. `width`/`height` are the negotiated capture dimensions.
pub struct CaptureHandle {
    pub width: u32,
    pub height: u32,
    stop: pipewire::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn stop(self) {
        // Drop does the work; this is just an explicit spelling.
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing the user-selected monitor. Pops the portal consent dialog on
/// first use, then blocks until PipeWire negotiates a format (so the caller
/// knows the frame dimensions before setting up an encoder). Frames stream to
/// `frames` until the returned handle is dropped.
pub fn start(frames: crossbeam_channel::Sender<VideoFrame>) -> Result<CaptureHandle> {
    let (dims_tx, dims_rx) = crossbeam_channel::bounded::<(u32, u32)>(1);
    let (stop_tx, stop_rx) = pipewire::channel::channel::<()>();

    let thread = std::thread::Builder::new()
        .name("sc-capture".into())
        .spawn(move || {
            if let Err(e) = pollster::block_on(portal::run(frames, dims_tx, stop_rx)) {
                tracing::error!(error = %e, "capture ended with error");
            }
        })
        .map_err(|e| Error::Pipewire(e.to_string()))?;

    match dims_rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok((width, height)) => Ok(CaptureHandle {
            width,
            height,
            stop: stop_tx,
            thread: Some(thread),
        }),
        Err(_) => {
            let _ = stop_tx.send(());
            let _ = thread.join();
            Err(Error::NoFormat)
        }
    }
}
