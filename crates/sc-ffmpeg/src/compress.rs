//! Shrink a clip to fit under a size limit (Discord's upload limits), by
//! re-encoding it at a bitrate worked out from its length. If the picture would be
//! starved at the clip's own size, it is scaled down first. Sound from every audio
//! track is mixed into one. The video is encoded on the GPU like normal clips.

use crate::{AacEncoder, Encoder, Error, Mp4Muxer, Result};
use ffmpeg_next as ff;
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::time::Timestamp;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Sound is always re-encoded as stereo AAC at this rate and bitrate.
const AUDIO_RATE: u32 = 48_000;
const AUDIO_KBPS: u32 = 96;
/// How much of the size limit the file may use, leaving room for the container.
const HEADROOM: f64 = 0.93;
/// Bits per pixel per frame below which H.264 looks blocky.
const MIN_BPP: f64 = 0.055;

/// What came out.
pub struct Compressed {
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub video_kbps: u32,
}

fn err(e: ff::Error) -> Error {
    Error::Ffmpeg(e.to_string())
}

/// The clip's frame rate (rounded), 30 if it is not stated.
fn source_fps(path: &Path) -> Result<u32> {
    let ictx = ff::format::input(&path).map_err(err)?;
    let s = ictx
        .streams()
        .best(ff::media::Type::Video)
        .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
    let r = s.avg_frame_rate();
    let fps = if r.denominator() > 0 {
        r.numerator() as f64 / r.denominator() as f64
    } else {
        30.0
    };
    Ok((fps.round() as u32).clamp(10, 240))
}

/// The largest picture (up to the original size) the bitrate can fill properly.
fn pick_size(w: u32, h: u32, fps: u32, video_kbps: f64) -> (u32, u32) {
    let mut heights = vec![h, 1080, 900, 720, 540, 480, 360];
    heights.retain(|&x| x <= h);
    heights.dedup();
    for &oh in &heights {
        let ow = ((w as u64 * oh as u64 / h.max(1) as u64) as u32) & !1;
        let bpp = video_kbps * 1000.0 / (ow as f64 * oh as f64 * fps as f64);
        if bpp >= MIN_BPP {
            return (ow.max(2), oh & !1);
        }
    }
    let oh = *heights.last().unwrap_or(&h);
    (
        ((w as u64 * oh as u64 / h.max(1) as u64) as u32) & !1,
        oh & !1,
    )
}

/// All audio tracks of `path`, mixed into one stereo f32 stream at [`AUDIO_RATE`].
fn decode_audio_mix(path: &Path, tracks: usize) -> Vec<f32> {
    let stop = Arc::new(AtomicBool::new(false));
    let streams: Vec<Vec<f32>> = (0..tracks)
        .map(|n| {
            let rx = crate::spawn_audio(path, 0.0, n, stop.clone());
            let mut all = Vec::new();
            for chunk in rx.iter() {
                all.extend_from_slice(&chunk);
            }
            all
        })
        .collect();
    let len = streams.iter().map(Vec::len).max().unwrap_or(0);
    let mut mix = vec![0.0f32; len];
    for s in &streams {
        for (m, v) in mix.iter_mut().zip(s) {
            *m += *v;
        }
    }
    for m in &mut mix {
        *m = m.clamp(-1.0, 1.0);
    }
    mix
}

/// Everything one encode pass needs, so decoded frames can be fed one at a time.
struct Pass {
    enc: Encoder,
    mux: Mp4Muxer,
    aac: Option<AacEncoder>,
    audio: Vec<f32>,
    audio_pos: usize,
    scaler: Option<ff::software::scaling::Context>,
    out: ff::frame::Video,
    w: u32,
    h: u32,
    tb: f64,
    first: Option<f64>,
    last_ns: i64,
}

impl Pass {
    /// Scale one decoded picture to NV12 at the target size and encode it, then
    /// feed the sound up to that moment so audio and video stay interleaved.
    fn video_frame(&mut self, frame: &ff::frame::Video) -> Result<()> {
        if self.scaler.is_none() {
            self.scaler = Some(
                ff::software::scaling::Context::get(
                    frame.format(),
                    frame.width(),
                    frame.height(),
                    ff::format::Pixel::NV12,
                    self.w,
                    self.h,
                    ff::software::scaling::Flags::BICUBIC,
                )
                .map_err(err)?,
            );
        }
        self.scaler
            .as_mut()
            .unwrap()
            .run(frame, &mut self.out)
            .map_err(err)?;
        let (w, h) = (self.w as usize, self.h as usize);
        let mut nv12 = Vec::with_capacity(w * h * 3 / 2);
        let (s0, s1) = (self.out.stride(0), self.out.stride(1));
        let (d0, d1) = (self.out.data(0), self.out.data(1));
        for row in 0..h {
            nv12.extend_from_slice(&d0[row * s0..row * s0 + w]);
        }
        for row in 0..h / 2 {
            nv12.extend_from_slice(&d1[row * s1..row * s1 + w]);
        }

        let pts = frame.pts().map(|p| p as f64 * self.tb).unwrap_or(0.0);
        let first = *self.first.get_or_insert(pts);
        let ns = (((pts - first).max(0.0)) * 1e9) as i64;
        let ns = ns.max(self.last_ns + 1);
        self.last_ns = ns;
        for p in self.enc.push_nv12(&nv12, ns)? {
            self.mux.write(&p)?;
        }
        self.feed_audio(ns as f64 / 1e9 + 0.1)
    }

    /// Encode sound up to `until` seconds.
    fn feed_audio(&mut self, until: f64) -> Result<()> {
        let Some(aac) = self.aac.as_mut() else {
            return Ok(());
        };
        let chunk = 1024 * 2;
        while self.audio_pos < self.audio.len() {
            let t = (self.audio_pos / 2) as f64 / AUDIO_RATE as f64;
            if t > until {
                break;
            }
            let end = (self.audio_pos + chunk).min(self.audio.len());
            let buf = AudioBuffer {
                role: AudioRole::Desktop,
                sample_rate: AUDIO_RATE,
                channels: 2,
                timestamp: Timestamp::from_nanos((t * 1e9) as i64),
                samples: self.audio[self.audio_pos..end].to_vec(),
            };
            self.audio_pos = end;
            for p in aac.push(&buf)? {
                self.mux.write_audio(&p)?;
            }
        }
        Ok(())
    }

    /// Flush both encoders and close the file.
    fn finish(mut self) -> Result<()> {
        self.feed_audio(f64::MAX)?;
        for p in self.enc.flush()? {
            self.mux.write(&p)?;
        }
        if let Some(aac) = self.aac.as_mut() {
            for p in aac.flush()? {
                self.mux.write_audio(&p)?;
            }
        }
        self.mux.finish()
    }
}

/// One encode of `src` into `dst` at the given picture size and video bitrate.
fn encode_once(
    src: &Path,
    dst: &Path,
    (w, h): (u32, u32),
    fps: u32,
    video_kbps: u32,
    audio: &[f32],
) -> Result<()> {
    let mut ictx = ff::format::input(&src).map_err(err)?;
    let (index, params, tb) = {
        let s = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
        let r = s.time_base();
        (
            s.index(),
            s.parameters(),
            r.numerator() as f64 / r.denominator().max(1) as f64,
        )
    };
    let mut decoder = ff::codec::context::Context::from_parameters(params)
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;

    let enc = Encoder::new(w, h, fps, video_kbps, fps * 2, false)?;
    let vparams = enc.codec_params()?;
    let aac = if audio.is_empty() {
        None
    } else {
        Some(AacEncoder::new(AUDIO_RATE, 2, AUDIO_KBPS)?)
    };
    let mux = match &aac {
        Some(a) => Mp4Muxer::with_audio(dst, &vparams, &a.codec_params()?, a.rate())?,
        None => Mp4Muxer::from_params(dst, &vparams)?,
    };
    let mut pass = Pass {
        enc,
        mux,
        aac,
        audio: audio.to_vec(),
        audio_pos: 0,
        scaler: None,
        out: ff::frame::Video::empty(),
        w,
        h,
        tb,
        first: None,
        last_ns: -1,
    };

    let mut frame = ff::frame::Video::empty();
    for (stream, packet) in ictx.packets() {
        if stream.index() != index || decoder.send_packet(&packet).is_err() {
            continue;
        }
        while decoder.receive_frame(&mut frame).is_ok() {
            pass.video_frame(&frame)?;
        }
    }
    let _ = decoder.send_eof();
    while decoder.receive_frame(&mut frame).is_ok() {
        pass.video_frame(&frame)?;
    }
    pass.finish()
}

/// Re-encode `src` into `dst`, at most `target_bytes` big. Fails if the clip is
/// already small enough, or cannot be squeezed under the limit.
pub fn compress_to_size(src: &Path, dst: &Path, target_bytes: u64) -> Result<Compressed> {
    crate::init()?;
    let info = crate::media_info(src)?;
    let size = std::fs::metadata(src)
        .map_err(|e| Error::Ffmpeg(e.to_string()))?
        .len();
    if size <= target_bytes {
        return Err(Error::Ffmpeg(
            "this clip is already under that size, so it can be shared as it is".into(),
        ));
    }
    let fps = source_fps(src)?;
    let audio = if info.audio_streams > 0 {
        decode_audio_mix(src, info.audio_streams)
    } else {
        Vec::new()
    };
    let secs = info.duration.max(0.5);
    let audio_kbps = if audio.is_empty() {
        0.0
    } else {
        AUDIO_KBPS as f64
    };
    let mut video_kbps =
        ((target_bytes as f64 * 8.0 * HEADROOM) / secs / 1000.0 - audio_kbps).max(120.0);

    for _ in 0..3 {
        let (w, h) = pick_size(info.width, info.height, fps, video_kbps);
        let _ = std::fs::remove_file(dst);
        encode_once(src, dst, (w, h), fps, video_kbps as u32, &audio)?;
        let got = std::fs::metadata(dst)
            .map_err(|e| Error::Ffmpeg(e.to_string()))?
            .len();
        if got <= target_bytes {
            return Ok(Compressed {
                bytes: got,
                width: w,
                height: h,
                video_kbps: video_kbps as u32,
            });
        }
        // Over the limit: aim lower by how much it overshot, with a little extra.
        video_kbps = (video_kbps * target_bytes as f64 / got as f64 * 0.95).max(80.0);
    }
    let _ = std::fs::remove_file(dst);
    Err(Error::Ffmpeg(
        "couldn't get this clip under that size; try a larger limit".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_bitrates_pick_smaller_pictures() {
        // Plenty of bits keep the original size.
        assert_eq!(pick_size(1920, 1080, 60, 12_000.0), (1920, 1080));
        // Starved bits step down.
        let (_, h) = pick_size(1920, 1080, 60, 1_200.0);
        assert!(h < 1080);
        // Never larger than the original.
        let (w, h) = pick_size(1280, 720, 30, 50_000.0);
        assert_eq!((w, h), (1280, 720));
        // Even sides always.
        let (w, h) = pick_size(1920, 1080, 60, 300.0);
        assert!(w % 2 == 0 && h % 2 == 0);
    }
}
