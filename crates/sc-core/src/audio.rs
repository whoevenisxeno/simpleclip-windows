use crate::time::Timestamp;
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_monitor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioRole {
    /// System/desktop output (WASAPI loopback / PipeWire sink-monitor).
    Desktop,
    /// Optional microphone input.
    Microphone,
}

/// Interleaved f32 samples with a monotonic timestamp on the same timebase as
/// video, so the muxer can keep A/V drift under the 50ms acceptance bound.
pub struct AudioBuffer {
    pub role: AudioRole,
    pub sample_rate: u32,
    pub channels: u16,
    pub timestamp: Timestamp,
    pub samples: Vec<f32>,
}

/// One implementation per platform audio system. Desktop and mic are separate
/// sources; mixing/track layout is decided at save time, not here.
pub trait AudioSource: Send {
    fn role(&self) -> AudioRole;
    fn enumerate(&self) -> Result<Vec<AudioDevice>>;
    fn start(
        &mut self,
        device_id: &str,
        sink: crossbeam_channel::Sender<AudioBuffer>,
    ) -> Result<Box<dyn AudioSession>>;
}

pub trait AudioSession: Send {
    fn stop(self: Box<Self>);
}
