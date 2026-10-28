use crate::time::Timestamp;
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EncoderKind {
    Nvenc,
    Vaapi,
    Qsv,
    Amf,
    Software,
}

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub gop_frames: u32,
}

#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub timestamp: Timestamp,
    pub duration: i64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

pub trait Encoder: Send {
    fn kind(&self) -> EncoderKind;
    fn codec(&self) -> Codec;
    fn encode(&mut self, frame: &crate::capture::VideoFrame) -> Result<Vec<EncodedPacket>>;
    fn flush(&mut self) -> Result<Vec<EncodedPacket>>;
}
