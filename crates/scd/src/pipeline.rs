use crate::audio_mix::{apply_volume, Mixer, Track};
use anyhow::{bail, Context, Result};
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::buffer::PacketRing;
use sc_core::config::{AudioTracks, Config};
use sc_core::encode::EncodedPacket;
use sc_ffmpeg::{AacEncoder, Bgra2Nv12, CodecParams, Encoder, Mp4Muxer};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub struct Pipeline {
    video_ring: Arc<Mutex<PacketRing>>,
    audio_tracks: Vec<Arc<AudioTrack>>,
    video_params: Arc<CodecParams>,
    pub width: u32,
    pub height: u32,
    pub encoder: sc_core::encode::EncoderKind,
    paused: Arc<AtomicBool>,
    volumes: Arc<Volumes>,
    sync_ns: AtomicI64,
    _capture: sc_capture::CaptureHandle,
    _video_convert: JoinHandle<()>,
    _video_encode: JoinHandle<()>,
    _audio_encode: JoinHandle<()>,
}

struct Volumes {
    desktop: AtomicU32,
    mic: AtomicU32,
}

impl Volumes {
    fn of(&self, role: AudioRole) -> u32 {
        match role {
            AudioRole::Desktop => self.desktop.load(Ordering::Relaxed),
            AudioRole::Microphone => self.mic.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone)]
struct AudioMeta {
    params: Arc<CodecParams>,
    rate: i32,
}

struct AudioTrack {
    ring: Mutex<PacketRing>,
    meta: Mutex<Option<AudioMeta>>,
}

#[derive(Clone, Copy, PartialEq)]
enum AudioMode {
    Single(AudioRole),
    Mixed,
    Separate,
}

impl AudioMode {
    fn from_config(cfg: &Config) -> Self {
        let mic = cfg!(windows) && cfg.audio.mic_enabled;
        match (cfg.audio.desktop_enabled, mic) {
            (true, true) if cfg.audio.tracks == AudioTracks::Separate => Self::Separate,
            (true, true) => Self::Mixed,
            (false, true) => Self::Single(AudioRole::Microphone),
            _ => Self::Single(AudioRole::Desktop),
        }
    }

    fn track_count(self) -> usize {
        if self == Self::Separate {
            2
        } else {
            1
        }
    }
}

const AUDIO_KBPS: u32 = 160;

fn output_size(w: u32, h: u32, max_h: Option<u32>) -> (u32, u32) {
    match max_h {
        Some(m) if m < h => {
            let oh = m & !1;
            let ow = ((w as u64 * oh as u64 / h as u64) as u32) & !1;
            (ow.max(2), oh.max(2))
        }
        _ => (w, h),
    }
}

fn ring_bytes(kbps: u64, secs: u64) -> usize {
    let bytes = (kbps * 1000 / 8) * secs;
    (bytes + bytes / 2) as usize
}

impl Pipeline {
    pub fn start(cfg: &Config) -> Result<Self> {
        let (vtx, vrx) = crossbeam_channel::bounded(4);
        let (atx, arx) = crossbeam_channel::unbounded();
        let mode = AudioMode::from_config(cfg);
        let want_audio = cfg.audio.desktop_enabled || (cfg!(windows) && cfg.audio.mic_enabled);
        let audio_tx = want_audio.then_some(atx);
        let opts = sc_capture::Options {
            monitor_id: cfg.capture.monitor_id.clone(),
            show_cursor: cfg.capture.show_cursor,
            fps: cfg.capture.target_fps,
            desktop_audio: cfg.audio.desktop_enabled,
            audio_device: cfg.audio.desktop_device.clone(),
            mic_enabled: cfg!(windows) && cfg.audio.mic_enabled,
            mic_device: cfg.audio.mic_device.clone().unwrap_or_default(),
        };
        let cap = sc_capture::start(vtx, audio_tx, &opts).context("starting capture")?;
        let (src_w, src_h) = (cap.width, cap.height);
        let (width, height) = output_size(src_w, src_h, cfg.encode.max_height);
        let scaling = (width, height) != (src_w, src_h);
        let paused = Arc::new(AtomicBool::new(false));
        let fps = cfg.capture.target_fps.max(1);
        let secs = cfg.buffer.replay_duration_secs as u64;

        let mut venc = Encoder::new(
            width,
            height,
            fps,
            cfg.encode.bitrate_kbps,
            cfg.encode.gop_frames,
            cfg.encode.gpu_convert && !scaling,
        )
        .context("opening video encoder")?;
        let encoder_kind = venc.kind();
        let gpu_convert = venc.takes_bgra();
        let video_params = Arc::new(venc.codec_params().context("video params")?);
        let video_ring = Arc::new(Mutex::new(PacketRing::new(ring_bytes(
            cfg.encode.bitrate_kbps as u64,
            secs,
        ))));
        let mut conv = if gpu_convert {
            None
        } else {
            Some(Bgra2Nv12::new_scaled(src_w, src_h, width, height).context("converter")?)
        };
        let vring = video_ring.clone();
        let vpaused = paused.clone();

        let (ntx, nrx) = crossbeam_channel::bounded::<(sc_core::capture::FrameBuf, i64, usize)>(2);
        let video_convert = std::thread::Builder::new()
            .name("sc-vconv".into())
            .spawn(move || {
                for frame in vrx.iter() {
                    if vpaused.load(Ordering::Relaxed) {
                        continue;
                    }
                    let ts = frame.timestamp.as_nanos();
                    let item = match conv.as_mut() {
                        Some(c) => match c.convert(&frame.data, frame.stride as usize) {
                            Ok(nv12) => (nv12.into(), ts, 0),
                            Err(e) => {
                                tracing::debug!(error = %e, "frame convert failed");
                                continue;
                            }
                        },
                        None => (frame.data, ts, frame.stride as usize),
                    };
                    if ntx.send(item).is_err() {
                        break;
                    }
                }
            })
            .context("video convert thread")?;
        let video_encode = std::thread::Builder::new()
            .name("sc-venc".into())
            .spawn(move || {
                let (mut frames_in, mut packets_out, mut errors) = (0u32, 0u32, 0u32);
                let mut spent = std::time::Duration::ZERO;
                let mut report = std::time::Instant::now();
                for (pixels, ts, stride) in nrx.iter() {
                    let t = std::time::Instant::now();
                    frames_in += 1;
                    let pushed = if stride == 0 {
                        venc.push_nv12(&pixels, ts)
                    } else {
                        venc.push_bgra(&pixels, stride, ts)
                    };
                    match pushed {
                        Ok(pkts) => {
                            packets_out += pkts.len() as u32;
                            let mut r = vring.lock().unwrap();
                            for p in pkts {
                                r.push(p);
                            }
                        }
                        Err(e) => {
                            errors += 1;
                            tracing::debug!(error = %e, "video encode failed");
                        }
                    }
                    spent += t.elapsed();
                    if report.elapsed() >= std::time::Duration::from_secs(5) {
                        tracing::debug!(
                            frames_in,
                            packets_out,
                            errors,
                            avg_ms = spent.as_secs_f64() * 1000.0 / frames_in.max(1) as f64,
                            "video encode, last 5s"
                        );
                        (frames_in, packets_out, errors) = (0, 0, 0);
                        (spent, report) = (std::time::Duration::ZERO, std::time::Instant::now());
                    }
                }
            })
            .context("video encode thread")?;

        let audio_tracks: Vec<Arc<AudioTrack>> = (0..mode.track_count())
            .map(|_| {
                Arc::new(AudioTrack {
                    ring: Mutex::new(PacketRing::new(ring_bytes(AUDIO_KBPS as u64, secs))),
                    meta: Mutex::new(None),
                })
            })
            .collect();
        let volumes = Arc::new(Volumes {
            desktop: AtomicU32::new(cfg.audio.desktop_volume),
            mic: AtomicU32::new(cfg.audio.mic_volume),
        });
        let (tracks, apaused, avolumes) = (audio_tracks.clone(), paused.clone(), volumes.clone());
        let audio_encode = std::thread::Builder::new()
            .name("sc-aenc".into())
            .spawn(move || audio_loop(arx, tracks, mode, apaused, avolumes))
            .context("audio encode thread")?;

        tracing::info!(
            width,
            height,
            fps,
            audio_tracks = audio_tracks.len(),
            mic = cfg!(windows) && cfg.audio.mic_enabled,
            "capture pipeline running"
        );
        Ok(Self {
            video_ring,
            audio_tracks,
            video_params,
            width,
            height,
            encoder: encoder_kind,
            paused,
            volumes,
            sync_ns: AtomicI64::new(cfg.audio.sync_offset_ms as i64 * 1_000_000),
            _capture: cap,
            _video_convert: video_convert,
            _video_encode: video_encode,
            _audio_encode: audio_encode,
        })
    }
}

fn audio_loop(
    rx: crossbeam_channel::Receiver<AudioBuffer>,
    tracks: Vec<Arc<AudioTrack>>,
    mode: AudioMode,
    paused: Arc<AtomicBool>,
    volumes: Arc<Volumes>,
) {
    let mut encoders: Vec<Option<AacEncoder>> = tracks.iter().map(|_| None).collect();
    let mut lanes: Vec<Track> = tracks.iter().map(|_| Track::default()).collect();
    let mut mixer = Mixer::default();
    for mut buf in rx.iter() {
        if buf.sample_rate == 0 || paused.load(Ordering::Relaxed) {
            continue;
        }
        apply_volume(&mut buf.samples, volumes.of(buf.role));
        match mode {
            AudioMode::Mixed => {
                mixer.push(&buf);
                while let Some(out) = mixer.pop() {
                    encode_into(&tracks[0], &mut encoders[0], &out);
                }
            }
            AudioMode::Separate => {
                let i = usize::from(buf.role == AudioRole::Microphone);
                if let Some(aligned) = lanes[i].push(&buf) {
                    encode_into(&tracks[i], &mut encoders[i], &aligned);
                }
            }
            AudioMode::Single(role) => {
                if buf.role == role {
                    if let Some(aligned) = lanes[0].push(&buf) {
                        encode_into(&tracks[0], &mut encoders[0], &aligned);
                    }
                }
            }
        }
    }
}

fn encode_into(track: &AudioTrack, enc: &mut Option<AacEncoder>, buf: &AudioBuffer) {
    if enc.is_none() {
        match AacEncoder::new(buf.sample_rate, buf.channels, AUDIO_KBPS) {
            Ok(e) => {
                if let Ok(p) = e.codec_params() {
                    *track.meta.lock().unwrap() = Some(AudioMeta {
                        params: Arc::new(p),
                        rate: e.rate(),
                    });
                }
                *enc = Some(e);
            }
            Err(err) => {
                tracing::warn!(error = %err, "aac init failed");
                return;
            }
        }
    }
    if let Some(e) = enc.as_mut() {
        if let Ok(pkts) = e.push(buf) {
            let mut r = track.ring.lock().unwrap();
            for p in pkts {
                r.push(p);
            }
        }
    }
}

fn merge_write(
    video: &[EncodedPacket],
    audio: &[Vec<EncodedPacket>],
    mux: &mut Mp4Muxer,
) -> Result<()> {
    let mut vi = 0usize;
    let mut ai = vec![0usize; audio.len()];
    loop {
        let next_audio = audio
            .iter()
            .enumerate()
            .filter_map(|(t, a)| a.get(ai[t]).map(|p| (t, p.timestamp)))
            .min_by_key(|&(_, ts)| ts);
        match (video.get(vi), next_audio) {
            (None, None) => break,
            (Some(v), Some((_, ats))) if v.timestamp > ats => {
                let (t, _) = next_audio.unwrap();
                mux.write_audio_track(t, &audio[t][ai[t]])
                    .context("write audio packet")?;
                ai[t] += 1;
            }
            (Some(v), _) => {
                mux.write(v).context("write video packet")?;
                vi += 1;
            }
            (None, Some((t, _))) => {
                mux.write_audio_track(t, &audio[t][ai[t]])
                    .context("write audio packet")?;
                ai[t] += 1;
            }
        }
    }
    Ok(())
}

fn clip_audio(ring: &PacketRing, start: i64, offset_ns: i64) -> Vec<EncodedPacket> {
    ring.snapshot_since(start.saturating_sub(offset_ns))
        .into_iter()
        .filter_map(|mut p| {
            let ts = p.timestamp.as_nanos() + offset_ns;
            if ts < start {
                return None;
            }
            p.timestamp = sc_core::time::Timestamp::from_nanos(ts);
            Some(p)
        })
        .collect()
}

fn write_clip(
    video: &[EncodedPacket],
    video_params: &CodecParams,
    audio: &[(AudioMeta, Vec<EncodedPacket>)],
    path: &Path,
) -> Result<f64> {
    let (Some(first), Some(last)) = (video.first(), video.last()) else {
        bail!("replay buffer is empty");
    };
    let params: Vec<(&CodecParams, i32)> =
        audio.iter().map(|(m, _)| (&*m.params, m.rate)).collect();
    let mut mux = if params.is_empty() {
        Mp4Muxer::from_params(path, video_params).context("open muxer")?
    } else {
        Mp4Muxer::with_audio_tracks(path, video_params, &params).context("open muxer")?
    };
    let tracks: Vec<Vec<EncodedPacket>> = audio.iter().map(|(_, p)| p.clone()).collect();
    merge_write(video, &tracks, &mut mux)?;
    mux.finish().context("finish mp4")?;
    Ok((last.timestamp.as_nanos() - first.timestamp.as_nanos()) as f64 / 1_000_000_000.0)
}

impl Pipeline {
    pub fn set_volumes(&self, desktop: u32, mic: u32) {
        self.volumes.desktop.store(desktop, Ordering::Relaxed);
        self.volumes.mic.store(mic, Ordering::Relaxed);
    }

    pub fn save(&self, secs: u32, path: &Path) -> Result<f64> {
        let video = self.video_ring.lock().unwrap().snapshot(secs);
        if video.is_empty() {
            bail!("replay buffer is empty");
        }
        let start = video.first().unwrap().timestamp.as_nanos();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let offset = self.sync_ns.load(Ordering::Relaxed);
        let mut parts = Vec::new();
        for t in &self.audio_tracks {
            if let Some(m) = t.meta.lock().unwrap().clone() {
                let pkts = clip_audio(&t.ring.lock().unwrap(), start, offset);
                if !pkts.is_empty() {
                    parts.push((m, pkts));
                }
            }
        }
        write_clip(&video, &self.video_params, &parts, path)
    }

    pub fn set_sync_offset_ms(&self, ms: i32) {
        self.sync_ns.store(ms as i64 * 1_000_000, Ordering::Relaxed);
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    #[cfg(windows)]
    pub fn screenshot(&self, path: &Path) -> Result<()> {
        let frame = self
            ._capture
            .latest_frame()
            .context("no frame captured yet")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let (w, h, stride) = (
            frame.width as usize,
            frame.height as usize,
            frame.stride as usize,
        );
        let mut rgb = Vec::with_capacity(w * h * 3);
        for row in frame.data.chunks_exact(stride).take(h) {
            for px in row[..w * 4].as_chunks::<4>().0 {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        let file = std::io::BufWriter::new(std::fs::File::create(path).context("create png")?);
        let mut enc = png::Encoder::new(file, frame.width, frame.height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()
            .context("png header")?
            .write_image_data(&rgb)
            .context("png data")?;
        Ok(())
    }

    pub fn buffer_fill(&self) -> f32 {
        self.video_ring.lock().unwrap().fill()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::time::Timestamp;
    use sc_ffmpeg::{first_pts, frame_at, spawn_audio};

    const FPS: i64 = 30;
    const W: u32 = 160;
    const H: u32 = 90;

    fn video_packets(
        first_ms: i64,
        secs: i64,
        event_ms: i64,
    ) -> Option<(Vec<EncodedPacket>, CodecParams)> {
        let mut enc = Encoder::new(W, H, FPS as u32, 2000, FPS as u32, false).ok()?;
        let params = enc.codec_params().ok()?;
        let mut out = Vec::new();
        for n in 0..secs * FPS {
            let ts = first_ms * 1_000_000 + n * 1_000_000_000 / FPS;
            let luma = if ts / 1_000_000 >= event_ms { 200 } else { 40 };
            let mut nv12 = vec![128u8; (W * H * 3 / 2) as usize];
            nv12[..(W * H) as usize].fill(luma);
            out.extend(enc.push_nv12(&nv12, ts).ok()?);
        }
        out.extend(enc.flush().ok()?);
        Some((out, params))
    }

    fn audio_packets(
        first_ms: i64,
        secs: i64,
        event_ms: i64,
    ) -> Option<(AudioMeta, Vec<EncodedPacket>)> {
        let mut enc = AacEncoder::new(48_000, 2, 128).ok()?;
        let meta = AudioMeta {
            params: Arc::new(enc.codec_params().ok()?),
            rate: enc.rate(),
        };
        let mut out = Vec::new();
        for i in 0..secs * 100 {
            let ts = first_ms * 1_000_000 + i * 10_000_000;
            let loud = ts / 1_000_000 >= event_ms;
            let samples: Vec<f32> = (0..480)
                .flat_map(|k| {
                    let n = (i * 480 + k) as f32;
                    let v = if loud {
                        (2.0 * std::f32::consts::PI * 440.0 * n / 48_000.0).sin() * 0.5
                    } else {
                        0.0
                    };
                    [v, v]
                })
                .collect();
            let buf = AudioBuffer {
                role: AudioRole::Desktop,
                sample_rate: 48_000,
                channels: 2,
                timestamp: Timestamp::from_nanos(ts),
                samples,
            };
            out.extend(enc.push(&buf).ok()?);
        }
        out.extend(enc.flush().ok()?);
        Some((meta, out))
    }

    fn picture_event(path: &Path) -> f64 {
        let mut t = 0.0;
        while t < 8.0 {
            let (_, _, rgb) = frame_at(path, t, 64).unwrap();
            let mean = rgb.iter().map(|v| *v as f64).sum::<f64>() / rgb.len() as f64;
            if mean > 110.0 {
                return t;
            }
            t += 1.0 / 30.0;
        }
        panic!("the picture never changes");
    }

    fn sound_event(path: &Path) -> f64 {
        let (_, audio_start) = first_pts(path).unwrap();
        let lead = audio_start.unwrap_or(0.0);
        let stop = Arc::new(AtomicBool::new(false));
        let rx = spawn_audio(path, 0.0, 0, stop);
        let mut at = 0usize;
        for chunk in rx.iter() {
            if let Some(i) = chunk.iter().position(|v| v.abs() > 0.1) {
                return lead + (at + i) as f64 / 2.0 / 48_000.0;
            }
            at += chunk.len();
        }
        panic!("the sound never starts");
    }

    fn scenario(name: &str, first_ms: i64, offset_ms: i64) -> Option<(f64, f64)> {
        let (video, params) = video_packets(first_ms, 4, 2_000)?;
        let (meta, audio) = audio_packets(first_ms, 4, 2_000)?;
        let mut ring = PacketRing::new(usize::MAX / 4);
        for p in audio {
            ring.push(p);
        }
        let start = video.first().unwrap().timestamp.as_nanos();
        let offset = offset_ms * 1_000_000;
        let parts = vec![(meta, clip_audio(&ring, start, offset))];
        let dir = std::env::temp_dir().join(format!("sc-sync-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        write_clip(&video, &params, &parts, &path).unwrap();
        let out = (picture_event(&path), sound_event(&path));
        let _ = std::fs::remove_dir_all(&dir);
        Some(out)
    }

    #[test]
    fn sound_and_picture_line_up_whenever_capture_started_late() {
        for (name, first_ms) in [("a", 0), ("b", 120), ("c", 700), ("d", 2_300)] {
            let Some((picture, sound)) = scenario(name, first_ms, 0) else {
                return;
            };
            assert!(
                (sound - picture).abs() < 0.09,
                "first frame at {first_ms} ms: picture {picture:.3}s, sound {sound:.3}s"
            );
        }
    }

    #[test]
    fn the_sync_offset_moves_the_sound_later_or_earlier() {
        let Some((picture, base)) = scenario("base", 700, 0) else {
            return;
        };
        let (_, later) = scenario("later", 700, 250).unwrap();
        let (_, earlier) = scenario("earlier", 700, -250).unwrap();
        assert!(((later - base) - 0.25).abs() < 0.04, "{later} vs {base}");
        assert!(
            ((earlier - base) + 0.25).abs() < 0.04,
            "{earlier} vs {base}"
        );
        assert!((base - picture).abs() < 0.09);
    }

    #[test]
    fn audio_selection_follows_the_offset_and_never_precedes_the_picture() {
        let (meta, audio) = match audio_packets(0, 3, 0) {
            Some(a) => a,
            None => return,
        };
        let _ = meta;
        let mut ring = PacketRing::new(usize::MAX / 4);
        for p in audio {
            ring.push(p);
        }
        let start = 1_000_000_000;
        let plain = clip_audio(&ring, start, 0);
        assert!(plain.iter().all(|p| p.timestamp.as_nanos() >= start));
        let later = clip_audio(&ring, start, 300_000_000);
        assert!(later.iter().all(|p| p.timestamp.as_nanos() >= start));
        assert!(later.first().unwrap().timestamp.as_nanos() - start < 25_000_000);
        let earlier = clip_audio(&ring, start, -300_000_000);
        assert!(earlier.iter().all(|p| p.timestamp.as_nanos() >= start));
        assert!(earlier.len() < plain.len());
    }
}
