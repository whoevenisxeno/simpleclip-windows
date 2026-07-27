use anyhow::{bail, Context, Result};
use sc_core::buffer::PacketRing;
use sc_core::config::Config;
use sc_ffmpeg::{Bgra2Nv12, CodecParams, Encoder, Mp4Muxer};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The live capture pipeline: a capture thread feeds BGRA frames to an encode
/// thread, which converts to NV12, encodes on the GPU, and pushes packets into a
/// rolling ring buffer. `save` snapshots the ring and writes a clip without
/// disturbing capture.
pub struct Pipeline {
    ring: Arc<Mutex<PacketRing>>,
    params: Arc<CodecParams>,
    pub width: u32,
    pub height: u32,
    _capture: sc_capture::CaptureHandle,
    _encode: JoinHandle<()>,
}

fn ring_bytes(cfg: &Config) -> usize {
    // Hold the configured replay window plus 50% headroom for bitrate spikes.
    let secs = cfg.buffer.replay_duration_secs as u64;
    let bytes = (cfg.encode.bitrate_kbps as u64 * 1000 / 8) * secs;
    (bytes + bytes / 2) as usize
}

impl Pipeline {
    pub fn start(cfg: &Config) -> Result<Self> {
        let (tx, rx) = crossbeam_channel::bounded(8);
        let cap = sc_capture::start(tx).context("starting screen capture")?;
        let (width, height) = (cap.width, cap.height);
        let fps = cfg.capture.target_fps.max(1);

        let mut enc = Encoder::new(
            width,
            height,
            fps,
            cfg.encode.bitrate_kbps,
            cfg.encode.gop_frames,
        )
        .context("opening encoder")?;
        let params = Arc::new(enc.codec_params().context("codec params")?);
        let ring = Arc::new(Mutex::new(PacketRing::new(ring_bytes(cfg))));

        let ring2 = ring.clone();
        let mut conv = Bgra2Nv12::new(width, height).context("color converter")?;
        let encode = std::thread::Builder::new()
            .name("sc-encode".into())
            .spawn(move || {
                for frame in rx.iter() {
                    let nv12 = match conv.convert(&frame.data, frame.stride as usize) {
                        Ok(n) => n,
                        Err(e) => {
                            tracing::warn!(error = %e, "convert failed");
                            continue;
                        }
                    };
                    match enc.push_nv12(&nv12, frame.timestamp.as_nanos()) {
                        Ok(pkts) => {
                            let mut r = ring2.lock().unwrap();
                            for p in pkts {
                                r.push(p);
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "encode failed"),
                    }
                }
            })
            .context("spawning encode thread")?;

        tracing::info!(width, height, fps, "capture pipeline running");
        Ok(Self {
            ring,
            params,
            width,
            height,
            _capture: cap,
            _encode: encode,
        })
    }
}

impl Pipeline {
    /// Snapshot the last `secs` seconds and write them to `path`. Cloning the
    /// packets happens under the lock; the muxing runs after, so capture is never
    /// stalled by a save.
    pub fn save(&self, secs: u32, path: &Path) -> Result<f64> {
        let packets = self.ring.lock().unwrap().snapshot(secs);
        if packets.is_empty() {
            bail!("replay buffer is empty");
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let first = packets.first().unwrap().timestamp.as_nanos();
        let last = packets.last().unwrap().timestamp.as_nanos();
        let mut mux = Mp4Muxer::from_params(path, &self.params).context("open muxer")?;
        for p in &packets {
            mux.write(p).context("write packet")?;
        }
        mux.finish().context("finish mp4")?;
        Ok((last - first) as f64 / 1_000_000_000.0)
    }

    pub fn buffer_fill(&self) -> f32 {
        self.ring.lock().unwrap().fill()
    }
}
