// Linux portal + PipeWire capture backend. No-op on other targets.
#![cfg(target_os = "linux")]

mod audio;
mod portal;
mod stream;

use sc_core::audio::AudioBuffer;
use sc_core::capture::VideoFrame;
use std::thread::JoinHandle;
use std::time::Instant;

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

/// A running capture (video, plus desktop audio if requested). Dropping the
/// handle tears both PipeWire loops down cleanly. `width`/`height` are the
/// negotiated video dimensions.
pub struct CaptureHandle {
    pub width: u32,
    pub height: u32,
    stop: pipewire::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
    audio_stop: Option<pipewire::channel::Sender<()>>,
    audio_thread: Option<JoinHandle<()>>,
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(s) = &self.audio_stop {
            let _ = s.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.audio_thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing. Pops the portal consent dialog on first use, then blocks
/// until the video format is negotiated. If `audio` is provided, desktop audio
/// is captured on a shared monotonic clock so it stays in sync with video.
pub fn start(
    frames: crossbeam_channel::Sender<VideoFrame>,
    audio: Option<crossbeam_channel::Sender<AudioBuffer>>,
) -> Result<CaptureHandle> {
    let epoch = Instant::now();
    let (dims_tx, dims_rx) = crossbeam_channel::bounded::<(u32, u32)>(1);
    let (stop_tx, stop_rx) = pipewire::channel::channel::<()>();

    let thread = std::thread::Builder::new()
        .name("sc-capture-video".into())
        .spawn(move || {
            if let Err(e) = pollster::block_on(portal::run(frames, dims_tx, stop_rx, epoch)) {
                tracing::error!(error = %e, "video capture ended with error");
            }
        })
        .map_err(|e| Error::Pipewire(e.to_string()))?;

    let (audio_stop, audio_thread) = match audio {
        Some(tx) => {
            let (a_stop_tx, a_stop_rx) = pipewire::channel::channel::<()>();
            let t = std::thread::Builder::new()
                .name("sc-capture-audio".into())
                .spawn(move || {
                    if let Err(e) = audio::run(tx, epoch, a_stop_rx) {
                        tracing::warn!(error = %e, "audio capture ended with error");
                    }
                })
                .map_err(|e| Error::Pipewire(e.to_string()))?;
            (Some(a_stop_tx), Some(t))
        }
        None => (None, None),
    };

    match dims_rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok((width, height)) => Ok(CaptureHandle {
            width,
            height,
            stop: stop_tx,
            thread: Some(thread),
            audio_stop,
            audio_thread,
        }),
        Err(_) => {
            let _ = stop_tx.send(());
            let _ = thread.join();
            Err(Error::NoFormat)
        }
    }
}
