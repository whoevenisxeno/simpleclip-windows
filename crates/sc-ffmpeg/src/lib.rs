#![cfg(any(target_os = "linux", windows))]

mod audio_encoder;
mod compress;
mod convert;
mod edit;
mod encoder;
#[cfg(target_os = "linux")]
mod hw;
mod mux;
mod player;
mod recorder;
#[cfg(any(test, feature = "sample"))]
pub mod sample;
mod stretch;
mod thumb;
mod trim;

pub use audio_encoder::AacEncoder;
pub use compress::{compress_to_size, Compressed};
pub use convert::Bgra2Nv12;
pub use edit::{render as render_edit, Progress as EditProgress, Rendered, CANCELLED};
pub use encoder::{CodecParams, Encoder};
pub use mux::Mp4Muxer;
pub use player::{media_info, spawn_audio, spawn_video, MediaInfo, VideoFrame, AUDIO_RATE};
pub use recorder::Recorder;
pub use thumb::{frame_at, thumbnail};
pub use trim::{probe_duration, trim_copy};

use ffmpeg_next as ff;
use sc_core::encode::EncoderKind;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    #[error("no usable hardware encoder found for {codec}")]
    NoEncoder { codec: &'static str },
    #[error("{}", av_message(*code, ctx))]
    Av { code: i32, ctx: &'static str },
}

fn av_message(code: i32, ctx: &str) -> String {
    match code {
        -2 => "couldn't open the file; it may have been moved or deleted".into(),
        -13 => "couldn't open the file; permission denied".into(),
        -1_094_995_529 => "this file is not a valid video".into(),
        _ => format!("ffmpeg returned {code} at {ctx}"),
    }
}

pub fn init() -> Result<()> {
    ff::init().map_err(|e| Error::Ffmpeg(e.to_string()))
}

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
    #[cfg(windows)]
    (EncoderKind::Software, "libx264"),
    #[cfg(windows)]
    (EncoderKind::Software, "h264_mf"),
];

#[cfg(target_os = "linux")]
pub(crate) const ENCODE_ORDER: &[(EncoderKind, &str)] = &[(EncoderKind::Vaapi, "h264_vaapi")];
#[cfg(windows)]
pub(crate) const ENCODE_ORDER: &[(EncoderKind, &str)] = H264_CANDIDATES;

pub fn detect_h264_encoder() -> Option<(EncoderKind, &'static str)> {
    H264_CANDIDATES
        .iter()
        .find(|(_, name)| ff::encoder::find_by_name(name).is_some())
        .map(|(kind, name)| (*kind, *name))
}
