use anyhow::{bail, Context, Result};
use sc_core::buffer::PacketRing;
use sc_core::config::Config;
use sc_ffmpeg::{AacEncoder, Bgra2Nv12, CodecParams, Encoder, Mp4Muxer};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The live capture pipeline: capture threads feed video frames and desktop
/// audio to encode threads, which push encoded packets into rolling ring
/// buffers. `save` snapshots both rings and muxes a clip without disturbing
/// capture.
pub struct Pipeline {
    video_ring: Arc<Mutex<PacketRing>>,
    audio_ring: Arc<Mutex<PacketRing>>,
    video_params: Arc<CodecParams>,
    audio_meta: Arc<Mutex<Option<AudioMeta>>>,
    pub width: u32,
    pub height: u32,
    _capture: sc_capture::CaptureHandle,
    _video_encode: JoinHandle<()>,
    _audio_encode: JoinHandle<()>,
}

#[derive(Clone)]
struct AudioMeta {
    params: Arc<CodecParams>,
    rate: i32,
}

const AUDIO_KBPS: u32 = 160;

fn ring_bytes(kbps: u64, secs: u64) -> usize {
    let bytes = (kbps * 1000 / 8) * secs;
    (bytes + bytes / 2) as usize
}

impl Pipeline {
    pub fn start(cfg: &Config) -> Result<Self> {
        let (vtx, vrx) = crossbeam_channel::bounded(8);
        // Audio is tiny; never drop it (gaps cause audible artifacts), unlike video.
        let (atx, arx) = crossbeam_channel::unbounded();
        let audio_tx = cfg.audio.desktop_enabled.then_some(atx);
        let cap = sc_capture::start(vtx, audio_tx).context("starting capture")?;
        let (width, height) = (cap.width, cap.height);
        let fps = cfg.capture.target_fps.max(1);
        let secs = cfg.buffer.replay_duration_secs as u64;

        let mut venc = Encoder::new(
            width,
            height,
            fps,
            cfg.encode.bitrate_kbps,
            cfg.encode.gop_frames,
        )
        .context("opening video encoder")?;
        let video_params = Arc::new(venc.codec_params().context("video params")?);
        let video_ring = Arc::new(Mutex::new(PacketRing::new(ring_bytes(
            cfg.encode.bitrate_kbps as u64,
            secs,
        ))));
        let mut conv = Bgra2Nv12::new(width, height).context("converter")?;
        let vring = video_ring.clone();
        let video_encode = std::thread::Builder::new()
            .name("sc-venc".into())
            .spawn(move || {
                for frame in vrx.iter() {
                    let nv12 = match conv.convert(&frame.data, frame.stride as usize) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    if let Ok(pkts) = venc.push_nv12(&nv12, frame.timestamp.as_nanos()) {
                        let mut r = vring.lock().unwrap();
                        for p in pkts {
                            r.push(p);
                        }
                    }
                }
            })
            .context("video encode thread")?;

        let audio_ring = Arc::new(Mutex::new(PacketRing::new(ring_bytes(
            AUDIO_KBPS as u64,
            secs,
        ))));
        let audio_meta = Arc::new(Mutex::new(None));
        let (aring, ameta) = (audio_ring.clone(), audio_meta.clone());
        let audio_encode = std::thread::Builder::new()
            .name("sc-aenc".into())
            .spawn(move || audio_loop(arx, aring, ameta))
            .context("audio encode thread")?;

        tracing::info!(
            width,
            height,
            fps,
            "capture pipeline running (video + desktop audio)"
        );
        Ok(Self {
            video_ring,
            audio_ring,
            video_params,
            audio_meta,
            width,
            height,
            _capture: cap,
            _video_encode: video_encode,
            _audio_encode: audio_encode,
        })
    }
}

fn audio_loop(
    rx: crossbeam_channel::Receiver<sc_core::audio::AudioBuffer>,
    ring: Arc<Mutex<PacketRing>>,
    meta: Arc<Mutex<Option<AudioMeta>>>,
) {
    let mut enc: Option<AacEncoder> = None;
    for buf in rx.iter() {
        if buf.sample_rate == 0 {
            continue;
        }
        if enc.is_none() {
            match AacEncoder::new(buf.sample_rate, buf.channels, AUDIO_KBPS) {
                Ok(e) => {
                    if let Ok(p) = e.codec_params() {
                        *meta.lock().unwrap() = Some(AudioMeta {
                            params: Arc::new(p),
                            rate: e.rate(),
                        });
                    }
                    enc = Some(e);
                }
                Err(err) => {
                    tracing::warn!(error = %err, "aac init failed");
                    continue;
                }
            }
        }
        if let Some(e) = enc.as_mut() {
            if let Ok(pkts) = e.push(&buf) {
                let mut r = ring.lock().unwrap();
                for p in pkts {
                    r.push(p);
                }
            }
        }
    }
}

fn merge_write(
    video: &[sc_core::encode::EncodedPacket],
    audio: &[sc_core::encode::EncodedPacket],
    mux: &mut Mp4Muxer,
) -> Result<()> {
    let (mut vi, mut ai) = (0usize, 0usize);
    while vi < video.len() || ai < audio.len() {
        let take_v =
            ai >= audio.len() || (vi < video.len() && video[vi].timestamp <= audio[ai].timestamp);
        if take_v {
            mux.write(&video[vi]).context("write video packet")?;
            vi += 1;
        } else {
            mux.write_audio(&audio[ai]).context("write audio packet")?;
            ai += 1;
        }
    }
    Ok(())
}

impl Pipeline {
    /// Snapshot the last `secs` seconds of video and audio and mux them together.
    /// Cloning happens under the lock; muxing runs after, so capture is not stalled.
    pub fn save(&self, secs: u32, path: &Path) -> Result<f64> {
        let video = self.video_ring.lock().unwrap().snapshot(secs);
        if video.is_empty() {
            bail!("replay buffer is empty");
        }
        // Line the audio up with the video's keyframe start, not `secs` ago, so
        // the track doesn't begin partway into the clip.
        let start = video.first().unwrap().timestamp.as_nanos();
        let audio = self.audio_ring.lock().unwrap().snapshot_since(start);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let first = video.first().unwrap().timestamp.as_nanos();
        let last = video.last().unwrap().timestamp.as_nanos();

        let meta = self.audio_meta.lock().unwrap().clone();
        let mut mux = match &meta {
            Some(a) => Mp4Muxer::with_audio(path, &self.video_params, &a.params, a.rate)
                .context("open muxer")?,
            None => Mp4Muxer::from_params(path, &self.video_params).context("open muxer")?,
        };
        merge_write(&video, &audio, &mut mux)?;
        mux.finish().context("finish mp4")?;
        Ok((last - first) as f64 / 1_000_000_000.0)
    }

    pub fn buffer_fill(&self) -> f32 {
        self.video_ring.lock().unwrap().fill()
    }
}
