use crate::time::Timestamp;
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
    Nv12,
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub monitor_id: String,
    pub show_cursor: bool,
    pub target_fps: u32,
}

/// A single captured frame. In v1 `data` is a CPU buffer (download path); a GPU
/// handle variant is added later behind the same type as a zero-copy optimization.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub timestamp: Timestamp,
    pub data: Vec<u8>,
}

/// State surfaced to `sc status` and the tray icon. `NeedsConsent` is the
/// Wayland restore-token-failed case (§5.3): capture pauses, never yanks focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureState {
    Active,
    Paused,
    NeedsConsent,
    Stopped,
}

/// Enumerates sources and starts capture. One implementation per platform,
/// selected at runtime. Backends never touch the encoder directly; they push
/// frames to the sink they are handed at `start`.
pub trait CaptureBackend: Send {
    fn name(&self) -> &'static str;
    fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn start(
        &mut self,
        config: CaptureConfig,
        sink: crossbeam_channel::Sender<VideoFrame>,
    ) -> Result<Box<dyn CaptureSession>>;
}

/// Owns the running capture thread(s). Dropping it stops capture cleanly.
pub trait CaptureSession: Send {
    fn state(&self) -> CaptureState;
    fn stop(self: Box<Self>);
}
