use crate::{AacEncoder, Encoder, Error, Mp4Muxer, Result};
use ffmpeg_next as ff;
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::compress::CompressOptions;
use sc_core::time::Timestamp;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const AUDIO_RATE: u32 = 48_000;
const AUDIO_KBPS: u32 = 96;
const HEADROOM: f64 = 0.93;
const MIN_BPP: f64 = 0.055;
const MIN_FPS: u32 = 5;

#[derive(Debug)]
pub struct Compressed {
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub secs: f64,
    pub has_audio: bool,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressPlan {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
}

fn err(e: ff::Error) -> Error {
    Error::Ffmpeg(e.to_string())
}

pub fn source_fps(path: &Path) -> Result<u32> {
    crate::init()?;
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

pub fn source_facts(path: &Path) -> Result<Source> {
    let info = crate::media_info(path)?;
    Ok(Source {
        width: info.width,
        height: info.height,
        fps: source_fps(path)?,
        secs: info.duration,
        has_audio: info.audio_streams > 0,
        bytes: std::fs::metadata(path)
            .map_err(|e| Error::Ffmpeg(e.to_string()))?
            .len(),
    })
}

fn pick_size(w: u32, h: u32, fps: u32, video_kbps: f64, cap: Option<u32>) -> (u32, u32) {
    let limit = cap.map_or(h, |c| c.min(h));
    let mut heights = vec![h, 1080, 900, 720, 540, 480, 360];
    if let Some(c) = cap {
        heights.push(c);
    }
    heights.retain(|&x| x <= limit);
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    for &oh in &heights {
        let ow = ((w as u64 * oh as u64 / h.max(1) as u64) as u32) & !1;
        let bpp = video_kbps * 1000.0 / (ow as f64 * oh as f64 * fps as f64);
        if bpp >= MIN_BPP {
            return (ow.max(2), (oh & !1).max(2));
        }
    }
    let oh = *heights.last().unwrap_or(&limit);
    (
        (((w as u64 * oh as u64 / h.max(1) as u64) as u32) & !1).max(2),
        (oh & !1).max(2),
    )
}

pub fn plan_compress(src: &Source, opts: &CompressOptions) -> CompressPlan {
    let fps = opts
        .max_fps
        .map_or(src.fps, |m| src.fps.min(m))
        .clamp(MIN_FPS, 240);
    let audio_kbps = if src.has_audio && !opts.mute {
        AUDIO_KBPS
    } else {
        0
    };
    let secs = src.secs.max(0.5);
    let mut video = ((opts.target_bytes() as f64 * 8.0 * HEADROOM) / secs / 1000.0
        - audio_kbps as f64)
        .max(120.0);
    let source_video = (src.bytes as f64 * 8.0 / secs / 1000.0 - audio_kbps as f64).max(120.0);
    video = video.min(source_video);
    let (width, height) = pick_size(src.width, src.height, fps, video, opts.max_height);
    CompressPlan {
        width,
        height,
        fps,
        video_kbps: video as u32,
        audio_kbps,
    }
}

pub(crate) fn decode_audio_mix(path: &Path, tracks: usize) -> Vec<f32> {
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
    min_gap: f64,
    last_kept: Option<f64>,
}

impl Pass {
    fn video_frame(&mut self, frame: &ff::frame::Video) -> Result<()> {
        let at = frame.pts().map(|p| p as f64 * self.tb).unwrap_or(0.0);
        if self.last_kept.is_some_and(|last| at - last < self.min_gap) {
            return Ok(());
        }
        self.last_kept = Some(at);
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

fn encode_once(
    src: &Path,
    dst: &Path,
    (w, h): (u32, u32),
    fps: u32,
    min_gap: f64,
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
        min_gap,
        last_kept: None,
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

pub fn compress(src: &Path, dst: &Path, opts: &CompressOptions) -> Result<Compressed> {
    crate::init()?;
    opts.validate().map_err(Error::Ffmpeg)?;
    let facts = source_facts(src)?;
    let target_bytes = opts.target_bytes();
    if facts.bytes <= target_bytes && !opts.changes_picture_or_sound() {
        return Err(Error::Ffmpeg(
            "this clip is already under that size, so it can be shared as it is".into(),
        ));
    }
    let info = crate::media_info(src)?;
    let audio = if info.audio_streams > 0 && !opts.mute {
        decode_audio_mix(src, info.audio_streams)
    } else {
        Vec::new()
    };
    let mut plan = plan_compress(&facts, opts);
    if audio.is_empty() {
        plan.audio_kbps = 0;
    }
    let mut video_kbps = plan.video_kbps as f64;
    let fps = plan.fps;
    let min_gap = if fps < facts.fps {
        0.9 / fps as f64
    } else {
        0.0
    };

    for _ in 0..3 {
        let (w, h) = pick_size(facts.width, facts.height, fps, video_kbps, opts.max_height);
        let _ = std::fs::remove_file(dst);
        encode_once(src, dst, (w, h), fps, min_gap, video_kbps as u32, &audio)?;
        let got = std::fs::metadata(dst)
            .map_err(|e| Error::Ffmpeg(e.to_string()))?
            .len();
        if got <= target_bytes {
            return Ok(Compressed {
                bytes: got,
                width: w,
                height: h,
                fps,
                video_kbps: video_kbps as u32,
            });
        }
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
    use crate::sample::make_clip;

    fn facts(w: u32, h: u32, fps: u32, secs: f64, bytes: u64) -> Source {
        Source {
            width: w,
            height: h,
            fps,
            secs,
            has_audio: true,
            bytes,
        }
    }

    #[test]
    fn small_bitrates_pick_smaller_pictures() {
        assert_eq!(pick_size(1920, 1080, 60, 12_000.0, None), (1920, 1080));
        let (_, h) = pick_size(1920, 1080, 60, 1_200.0, None);
        assert!(h < 1080);
        let (w, h) = pick_size(1280, 720, 30, 50_000.0, None);
        assert_eq!((w, h), (1280, 720));
        let (w, h) = pick_size(1920, 1080, 60, 300.0, None);
        assert!(w % 2 == 0 && h % 2 == 0);
    }

    #[test]
    fn a_height_cap_limits_the_picture_even_when_there_is_room() {
        assert_eq!(pick_size(1920, 1080, 30, 50_000.0, Some(720)), (1280, 720));
        assert_eq!(pick_size(1920, 1080, 30, 50_000.0, Some(480)), (852, 480));
        assert_eq!(pick_size(1280, 720, 30, 50_000.0, Some(1080)), (1280, 720));
        let (_, h) = pick_size(1920, 1080, 30, 50_000.0, Some(600));
        assert_eq!(h, 600);
    }

    #[test]
    fn a_cap_and_a_tight_budget_still_shrink_to_fit() {
        let (_, h) = pick_size(1920, 1080, 60, 700.0, Some(720));
        assert!(h <= 480, "{h}");
    }

    #[test]
    fn the_plan_follows_the_size_and_the_caps() {
        let src = facts(1920, 1080, 60, 30.0, 120_000_000);
        let plain = plan_compress(&src, &CompressOptions::with_size(10));
        assert!(plain.height < 1080);
        assert_eq!(plain.fps, 60);
        assert_eq!(plain.audio_kbps, AUDIO_KBPS);

        let big = plan_compress(&src, &CompressOptions::with_size(500));
        assert_eq!((big.width, big.height), (1920, 1080));
        assert!(big.video_kbps > plain.video_kbps);

        let capped = plan_compress(
            &src,
            &CompressOptions {
                target_mb: 500,
                max_height: Some(720),
                max_fps: Some(30),
                mute: true,
            },
        );
        assert_eq!((capped.width, capped.height), (1280, 720));
        assert_eq!(capped.fps, 30);
        assert_eq!(capped.audio_kbps, 0);
    }

    #[test]
    fn a_frame_rate_cap_never_raises_the_rate() {
        let src = facts(1280, 720, 24, 10.0, 50_000_000);
        let plan = plan_compress(
            &src,
            &CompressOptions {
                max_fps: Some(60),
                ..CompressOptions::with_size(25)
            },
        );
        assert_eq!(plan.fps, 24);
    }

    #[test]
    fn muting_gives_the_picture_the_sounds_share() {
        let src = facts(1920, 1080, 30, 20.0, 80_000_000);
        let with = plan_compress(&src, &CompressOptions::with_size(10));
        let without = plan_compress(
            &src,
            &CompressOptions {
                mute: true,
                ..CompressOptions::with_size(10)
            },
        );
        assert!(without.video_kbps > with.video_kbps);
        assert!(without.video_kbps - with.video_kbps >= AUDIO_KBPS - 1);
        assert_eq!(without.audio_kbps, 0);
    }

    #[test]
    fn the_video_never_gets_more_bits_than_the_source_had() {
        let src = facts(1280, 720, 30, 10.0, 1_000_000);
        let plan = plan_compress(&src, &CompressOptions::with_size(500));
        assert!(plan.video_kbps <= 704, "{}", plan.video_kbps);
    }

    fn video_packets(path: &Path) -> usize {
        crate::init().unwrap();
        let mut ictx = ff::format::input(&path).unwrap();
        let index = ictx.streams().best(ff::media::Type::Video).unwrap().index();
        ictx.packets().filter(|(s, _)| s.index() == index).count()
    }

    fn workdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-compress-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_clip_that_already_fits_is_left_alone_unless_something_is_changed() {
        let dir = workdir("fits");
        let src = dir.join("clip.mp4");
        if !make_clip(&src, 4, 320, 180, true) {
            return;
        }
        let dst = dir.join("out.mp4");
        let err = compress(&src, &dst, &CompressOptions::with_size(50)).unwrap_err();
        assert!(err.to_string().contains("already under"));
        assert!(!dst.exists());
        let muted = CompressOptions {
            mute: true,
            ..CompressOptions::with_size(50)
        };
        let done = compress(&src, &dst, &muted).unwrap();
        assert!(done.bytes > 0 && dst.exists());
        assert_eq!(crate::media_info(&dst).unwrap().audio_streams, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_options_shape_the_saved_clip() {
        let dir = workdir("shape");
        let src = dir.join("clip.mp4");
        if !make_clip(&src, 6, 640, 360, true) {
            return;
        }
        let normal = dir.join("normal.mp4");
        let opts = CompressOptions {
            target_mb: 1,
            max_fps: Some(60),
            ..CompressOptions::default()
        };
        let done = compress(&src, &normal, &opts).unwrap();
        assert!(done.bytes <= 1_000_000);
        let info = crate::media_info(&normal).unwrap();
        assert_eq!(info.audio_streams, 1);
        let full_rate = video_packets(&normal);
        assert!(full_rate >= 170, "{full_rate}");

        let shaped = dir.join("shaped.mp4");
        let opts = CompressOptions {
            target_mb: 1,
            max_height: Some(180),
            max_fps: Some(15),
            mute: true,
        };
        let done = compress(&src, &shaped, &opts).unwrap();
        assert!(done.bytes <= 1_000_000);
        assert_eq!(done.height, 180);
        assert_eq!(done.fps, 15);
        let info = crate::media_info(&shaped).unwrap();
        assert_eq!((info.width, info.height), (320, 180));
        assert_eq!(info.audio_streams, 0);
        let halved = video_packets(&shaped);
        assert!(
            (80..=100).contains(&halved),
            "expected about 90 frames at 15 fps, got {halved}"
        );
        assert!((info.duration - 6.0).abs() < 0.3, "{}", info.duration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_options_are_refused_before_any_work() {
        let dir = workdir("bad");
        let dst = dir.join("never.mp4");
        let err = compress(
            Path::new("whatever.mp4"),
            &dst,
            &CompressOptions::with_size(0),
        )
        .unwrap_err();
        assert!(err.to_string().contains("between"));
        assert!(!dst.exists());
    }
}
