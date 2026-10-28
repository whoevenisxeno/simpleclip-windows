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
    Desktop,
    Microphone,
}

pub struct AudioBuffer {
    pub role: AudioRole,
    pub sample_rate: u32,
    pub channels: u16,
    pub timestamp: Timestamp,
    pub samples: Vec<f32>,
}

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
