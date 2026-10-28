#![cfg(any(target_os = "linux", windows))]

#[cfg(target_os = "linux")]
mod audio;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod stream;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use sc_core::audio::AudioBuffer;
#[cfg(target_os = "linux")]
use sc_core::capture::VideoFrame;
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(windows)]
pub use win::{list_audio_devices, list_monitors, start, CaptureHandle};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("portal: {0}")]
    Portal(String),
    #[error("pipewire: {0}")]
    Pipewire(String),
    #[error("capture ended before a format was negotiated")]
    NoFormat,
    #[error("windows capture: {0}")]
    Windows(String),
}

#[derive(Debug, Clone)]
pub struct Options {
    pub monitor_id: String,
    pub show_cursor: bool,
    pub fps: u32,
    pub desktop_audio: bool,
    pub audio_device: String,
    pub mic_enabled: bool,
    pub mic_device: String,
}

#[cfg(target_os = "linux")]
pub struct CaptureHandle {
    pub width: u32,
    pub height: u32,
    stop: pipewire::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
    audio_stop: Option<pipewire::channel::Sender<()>>,
    audio_thread: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub fn start(
    frames: crossbeam_channel::Sender<VideoFrame>,
    audio: Option<crossbeam_channel::Sender<AudioBuffer>>,
    _opts: &Options,
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
