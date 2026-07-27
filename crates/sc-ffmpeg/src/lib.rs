// Phase 1 encode path is Linux-only (system FFmpeg + VA-API). On other targets
// the crate compiles to nothing; the Windows encoder lands in Phase 4.
#![cfg(target_os = "linux")]

mod convert;
mod hw;
mod recorder;

pub use convert::Bgra2Nv12;
pub use recorder::Recorder;

use ffmpeg_next as ff;
use sc_core::encode::EncoderKind;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    #[error("no usable hardware encoder found for {codec}")]
    NoEncoder { codec: &'static str },
    #[error("ffmpeg returned {code} at {ctx}")]
    Av { code: i32, ctx: &'static str },
}

/// Initialize FFmpeg once. Cheap to call repeatedly.
pub fn init() -> Result<()> {
    ff::init().map_err(|e| Error::Ffmpeg(e.to_string()))
}

/// Probe hardware H.264 encoders in priority order and return the first whose
/// FFmpeg codec is present in this build. Actual device-open is verified when a
/// `Recorder` is constructed (§6.1: probe that it really initializes).
pub fn detect_h264_encoder() -> Option<(EncoderKind, &'static str)> {
    const CANDIDATES: &[(EncoderKind, &str)] = &[
        (EncoderKind::Nvenc, "h264_nvenc"),
        (EncoderKind::Vaapi, "h264_vaapi"),
        (EncoderKind::Qsv, "h264_qsv"),
        (EncoderKind::Amf, "h264_amf"),
    ];
    CANDIDATES
        .iter()
        .find(|(_, name)| ff::encoder::find_by_name(name).is_some())
        .map(|(kind, name)| (*kind, *name))
}
