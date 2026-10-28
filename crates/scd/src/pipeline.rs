use crate::audio_mix::{apply_volume, Mixer, Track};
use anyhow::{bail, Context, Result};
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::buffer::PacketRing;
use sc_core::config::{AudioTracks, Config};
use sc_core::encode::EncodedPacket;
use sc_ffmpeg::{AacEncoder, Bgra2Nv12, CodecParams, Encoder, Mp4Muxer};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
        let first = video.first().unwrap().timestamp.as_nanos();
        let last = video.last().unwrap().timestamp.as_nanos();

        let mut metas = Vec::new();
        let mut audio = Vec::new();
        for t in &self.audio_tracks {
            if let Some(m) = t.meta.lock().unwrap().clone() {
                let pkts = t.ring.lock().unwrap().snapshot_since(start);
                if !pkts.is_empty() {
                    metas.push(m);
                    audio.push(pkts);
                }
            }
        }
        let params: Vec<(&CodecParams, i32)> = metas.iter().map(|m| (&*m.params, m.rate)).collect();
        let mut mux = if params.is_empty() {
            Mp4Muxer::from_params(path, &self.video_params).context("open muxer")?
        } else {
            Mp4Muxer::with_audio_tracks(path, &self.video_params, &params).context("open muxer")?
        };
        merge_write(&video, &audio, &mut mux)?;
        mux.finish().context("finish mp4")?;
        Ok((last - first) as f64 / 1_000_000_000.0)
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
