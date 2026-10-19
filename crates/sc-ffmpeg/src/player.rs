//! Decoding for the in-app player. Video and audio are decoded on their own
//! threads (each with its own demuxer) and handed over through small bounded
//! channels, so a slow consumer simply makes the decoder wait. A decoder stops
//! when its `stop` flag is set or its receiver is dropped.

use crate::{Error, Result};
use crossbeam_channel::{bounded, Receiver, SendTimeoutError, Sender};
use ffmpeg_next as ff;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Audio is always delivered as interleaved stereo f32 at this rate.
pub const AUDIO_RATE: u32 = 48_000;

/// One decoded picture.
pub struct VideoFrame {
    /// Presentation time in seconds from the start of the clip.
    pub pts: f64,
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA.
    pub rgba: Vec<u8>,
}

fn err(e: ff::Error) -> Error {
    Error::Ffmpeg(e.to_string())
}

/// Send, giving up when asked to stop or when nobody is listening any more.
fn send<T>(tx: &Sender<T>, mut item: T, stop: &AtomicBool) -> bool {
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        match tx.send_timeout(item, Duration::from_millis(40)) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(back)) => item = back,
            Err(SendTimeoutError::Disconnected(_)) => return false,
        }
    }
}

/// Stream facts the player needs up front.
pub struct MediaInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}

pub fn media_info(path: &Path) -> Result<MediaInfo> {
    crate::init()?;
    let ictx = ff::format::input(&path).map_err(err)?;
    let v = ictx
        .streams()
        .best(ff::media::Type::Video)
        .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
    let dec = ff::codec::context::Context::from_parameters(v.parameters())
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;
    Ok(MediaInfo {
        duration: ictx.duration() as f64 / ff::ffi::AV_TIME_BASE as f64,
        width: dec.width(),
        height: dec.height(),
        has_audio: ictx.streams().best(ff::media::Type::Audio).is_some(),
    })
}

/// Start decoding video from `start` seconds. Frames before `start` are
/// dropped, and the picture is scaled down to at most `max_width` pixels wide.
pub fn spawn_video(
    path: &Path,
    start: f64,
    max_width: u32,
    stop: Arc<AtomicBool>,
) -> Receiver<VideoFrame> {
    let (tx, rx) = bounded(4);
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| {
            let _ = run_video(&path, start, max_width, &tx, &stop);
        });
    });
    rx
}

fn run_video(
    path: &PathBuf,
    start: f64,
    max_width: u32,
    tx: &Sender<VideoFrame>,
    stop: &AtomicBool,
) -> Result<()> {
    crate::init()?;
    let mut ictx = ff::format::input(path).map_err(err)?;
    let (index, params, tb, offset) = {
        let s = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
        let r = s.time_base();
        let tb = r.numerator() as f64 / r.denominator().max(1) as f64;
        let off = if s.start_time() > 0 {
            s.start_time() as f64 * tb
        } else {
            0.0
        };
        (s.index(), s.parameters(), tb, off)
    };
    let mut decoder = ff::codec::context::Context::from_parameters(params)
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;
    if start > 0.05 {
        let t = ((start + offset) * ff::ffi::AV_TIME_BASE as f64) as i64;
        let _ = ictx.seek(t, ..t);
    }

    let (sw, sh) = (decoder.width(), decoder.height());
    let tw = max_width.min(sw).max(2) & !1;
    let th = ((sh as u64 * tw as u64 / sw.max(1) as u64) as u32).max(2) & !1;
    let mut scaler: Option<ff::software::scaling::Context> = None;

    let mut frame = ff::frame::Video::empty();
    let mut rgba = ff::frame::Video::empty();
    let mut emit = |frame: &ff::frame::Video| -> Result<bool> {
        let pts = frame
            .pts()
            .map(|p| p as f64 * tb - offset)
            .unwrap_or(start)
            .max(0.0);
        if pts + 0.001 < start {
            return Ok(true);
        }
        if scaler.is_none() {
            scaler = Some(
                ff::software::scaling::Context::get(
                    frame.format(),
                    frame.width(),
                    frame.height(),
                    ff::format::Pixel::RGBA,
                    tw,
                    th,
                    ff::software::scaling::Flags::BILINEAR,
                )
                .map_err(err)?,
            );
        }
        scaler
            .as_mut()
            .unwrap()
            .run(frame, &mut rgba)
            .map_err(err)?;
        let stride = rgba.stride(0);
        let data = rgba.data(0);
        let mut out = Vec::with_capacity((tw * th * 4) as usize);
        for row in 0..th as usize {
            out.extend_from_slice(&data[row * stride..row * stride + tw as usize * 4]);
        }
        Ok(send(
            tx,
            VideoFrame {
                pts,
                width: tw,
                height: th,
                rgba: out,
            },
            stop,
        ))
    };

    for (stream, packet) in ictx.packets() {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if stream.index() != index || decoder.send_packet(&packet).is_err() {
            continue;
        }
        while decoder.receive_frame(&mut frame).is_ok() {
            if !emit(&frame)? {
                return Ok(());
            }
        }
    }
    let _ = decoder.send_eof();
    while decoder.receive_frame(&mut frame).is_ok() {
        if !emit(&frame)? {
            return Ok(());
        }
    }
    Ok(())
}

/// Start decoding audio from `start` seconds, resampled to stereo f32 at
/// [`AUDIO_RATE`]. Each message is a chunk of interleaved samples. The channel
/// closes at the end of the clip.
pub fn spawn_audio(path: &Path, start: f64, stop: Arc<AtomicBool>) -> Receiver<Vec<f32>> {
    let (tx, rx) = bounded(24);
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| {
            let _ = run_audio(&path, start, &tx, &stop);
        });
    });
    rx
}

fn run_audio(path: &PathBuf, start: f64, tx: &Sender<Vec<f32>>, stop: &AtomicBool) -> Result<()> {
    crate::init()?;
    let mut ictx = ff::format::input(path).map_err(err)?;
    let (index, params, tb, offset) = {
        let Some(s) = ictx.streams().best(ff::media::Type::Audio) else {
            return Ok(());
        };
        let r = s.time_base();
        let tb = r.numerator() as f64 / r.denominator().max(1) as f64;
        let off = if s.start_time() > 0 {
            s.start_time() as f64 * tb
        } else {
            0.0
        };
        (s.index(), s.parameters(), tb, off)
    };
    let mut decoder = ff::codec::context::Context::from_parameters(params)
        .map_err(err)?
        .decoder()
        .audio()
        .map_err(err)?;
    if start > 0.05 {
        let t = ((start + offset) * ff::ffi::AV_TIME_BASE as f64) as i64;
        let _ = ictx.seek(t, ..t);
    }

    let mut resampler: Option<ff::software::resampling::Context> = None;
    let mut frame = ff::frame::Audio::empty();
    let mut out = ff::frame::Audio::empty();

    let mut handle = |frame: &mut ff::frame::Audio| -> Result<bool> {
        let pts = frame.pts().map(|p| p as f64 * tb - offset);
        if pts.is_some_and(|p| p + frame.samples() as f64 / frame.rate().max(1) as f64 <= start) {
            return Ok(true);
        }
        if frame.channel_layout().is_empty() {
            frame.set_channel_layout(ff::ChannelLayout::default(frame.channels() as i32));
        }
        if resampler.is_none() {
            resampler = Some(
                ff::software::resampling::Context::get(
                    frame.format(),
                    frame.channel_layout(),
                    frame.rate(),
                    ff::format::Sample::F32(ff::format::sample::Type::Packed),
                    ff::ChannelLayout::STEREO,
                    AUDIO_RATE,
                )
                .map_err(err)?,
            );
        }
        resampler
            .as_mut()
            .unwrap()
            .run(frame, &mut out)
            .map_err(err)?;
        let n = out.samples() * 2;
        if n == 0 {
            return Ok(true);
        }
        let bytes = &out.data(0)[..n * 4];
        let chunk: Vec<f32> = (0..n)
            .map(|i| {
                f32::from_le_bytes([
                    bytes[i * 4],
                    bytes[i * 4 + 1],
                    bytes[i * 4 + 2],
                    bytes[i * 4 + 3],
                ])
            })
            .collect();
        Ok(send(tx, chunk, stop))
    };

    for (stream, packet) in ictx.packets() {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if stream.index() != index || decoder.send_packet(&packet).is_err() {
            continue;
        }
        while decoder.receive_frame(&mut frame).is_ok() {
            if !handle(&mut frame)? {
                return Ok(());
            }
        }
    }
    let _ = decoder.send_eof();
    while decoder.receive_frame(&mut frame).is_ok() {
        if !handle(&mut frame)? {
            return Ok(());
        }
    }
    Ok(())
}
