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

/// Hardware encoders in priority order; software is opt-in via build feature.
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
    /// GOP length in frames. Short (1-2s worth) so "last N seconds" starts on a
    /// keyframe and trims are clean.
    pub gop_frames: u32,
}

/// An encoded packet as stored in the ring buffer. Holds compressed bytes plus
/// the timing needed to mux later, not raw frames (raw 1080p60 blows the RAM
/// budget). Audio and video packets share the same monotonic timebase.
#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub timestamp: Timestamp,
    pub duration: i64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

/// Turns raw frames into encoded packets. One implementation per hardware
/// encoder, plus an opt-in software one. Detection probes that the encoder
/// actually initializes before committing to it.
pub trait Encoder: Send {
    fn kind(&self) -> EncoderKind;
    fn codec(&self) -> Codec;
    fn encode(&mut self, frame: &crate::capture::VideoFrame) -> Result<Vec<EncodedPacket>>;
    /// Flush any buffered packets at end-of-stream.
    fn flush(&mut self) -> Result<Vec<EncodedPacket>>;
}
