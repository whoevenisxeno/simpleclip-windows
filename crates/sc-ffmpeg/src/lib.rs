// Encode path: system FFmpeg. VA-API on Linux; NVENC / AMF / QSV (with a libx264
// fallback) on Windows. On other targets the crate compiles to nothing.
#![cfg(any(target_os = "linux", windows))]

mod audio_encoder;
mod convert;
mod encoder;
#[cfg(target_os = "linux")]
mod hw;
mod mux;
mod recorder;

pub use audio_encoder::AacEncoder;
pub use convert::Bgra2Nv12;
pub use encoder::{CodecParams, Encoder};
pub use mux::Mp4Muxer;
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

/// H.264 encoders in priority order for this platform. Hardware first; on
/// Windows the last entry is the software fallback so capture still works on a
/// machine with no usable GPU encoder.
pub(crate) const H264_CANDIDATES: &[(EncoderKind, &str)] = &[
    #[cfg(windows)]
    (EncoderKind::Nvenc, "h264_nvenc"),
    #[cfg(windows)]
    (EncoderKind::Amf, "h264_amf"),
    #[cfg(windows)]
    (EncoderKind::Qsv, "h264_qsv"),
    #[cfg(target_os = "linux")]
    (EncoderKind::Nvenc, "h264_nvenc"),
    #[cfg(target_os = "linux")]
    (EncoderKind::Vaapi, "h264_vaapi"),
    #[cfg(target_os = "linux")]
    (EncoderKind::Qsv, "h264_qsv"),
    #[cfg(target_os = "linux")]
    (EncoderKind::Amf, "h264_amf"),
    // Software fallbacks, tried only when no GPU encoder opens. libx264 needs a
    // GPL FFmpeg build; h264_mf is Windows' built-in Media Foundation encoder and
    // is present in LGPL builds too.
    #[cfg(windows)]
    (EncoderKind::Software, "libx264"),
    #[cfg(windows)]
    (EncoderKind::Software, "h264_mf"),
];

/// What `Encoder::new` actually tries, in order. Linux keeps its VA-API-only
/// path; Windows walks every candidate and keeps the first that opens.
#[cfg(target_os = "linux")]
pub(crate) const ENCODE_ORDER: &[(EncoderKind, &str)] = &[(EncoderKind::Vaapi, "h264_vaapi")];
#[cfg(windows)]
pub(crate) const ENCODE_ORDER: &[(EncoderKind, &str)] = H264_CANDIDATES;

/// Probe H.264 encoders in priority order and return the first whose FFmpeg
/// codec is present in this build. Actual device-open is verified when an
/// `Encoder` is constructed (§6.1: probe that it really initializes).
pub fn detect_h264_encoder() -> Option<(EncoderKind, &'static str)> {
    H264_CANDIDATES
        .iter()
        .find(|(_, name)| ff::encoder::find_by_name(name).is_some())
        .map(|(kind, name)| (*kind, *name))
}
