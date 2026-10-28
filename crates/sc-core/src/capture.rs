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

pub struct FrameBuf {
    buf: Vec<u8>,
    home: Option<crossbeam_channel::Sender<Vec<u8>>>,
}

impl From<Vec<u8>> for FrameBuf {
    fn from(buf: Vec<u8>) -> Self {
        Self { buf, home: None }
    }
}

impl std::ops::Deref for FrameBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buf
    }
}

impl Drop for FrameBuf {
    fn drop(&mut self) {
        if let Some(home) = &self.home {
            let _ = home.try_send(std::mem::take(&mut self.buf));
        }
    }
}

#[derive(Clone)]
pub struct FramePool {
    tx: crossbeam_channel::Sender<Vec<u8>>,
    rx: crossbeam_channel::Receiver<Vec<u8>>,
}

impl FramePool {
    pub fn new(max: usize) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(max);
        Self { tx, rx }
    }

    pub fn copy_of(&self, src: &[u8]) -> FrameBuf {
        let mut buf = self.rx.try_recv().unwrap_or_default();
        buf.clear();
        buf.extend_from_slice(src);
        FrameBuf {
            buf,
            home: Some(self.tx.clone()),
        }
    }
}

pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub timestamp: Timestamp,
    pub data: FrameBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureState {
    Active,
    Paused,
    NeedsConsent,
    Stopped,
}

pub trait CaptureBackend: Send {
    fn name(&self) -> &'static str;
    fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn start(
        &mut self,
        config: CaptureConfig,
        sink: crossbeam_channel::Sender<VideoFrame>,
    ) -> Result<Box<dyn CaptureSession>>;
}

pub trait CaptureSession: Send {
    fn state(&self) -> CaptureState;
    fn stop(self: Box<Self>);
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    #[test]
    fn dropped_buffers_are_reused() {
        let pool = FramePool::new(2);
        let first = pool.copy_of(&[1, 2, 3, 4]);
        let ptr = first.as_ptr();
        drop(first);
        let second = pool.copy_of(&[9, 9]);
        assert_eq!(&*second, &[9, 9]);
        assert_eq!(second.as_ptr(), ptr);
    }

    #[test]
    fn plain_vec_buffers_just_free() {
        let b: FrameBuf = vec![1u8, 2, 3].into();
        assert_eq!(b.len(), 3);
    }
}
