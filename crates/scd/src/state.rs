use sc_core::capture::CaptureState;
use sc_core::config::Config;
use sc_core::encode::EncoderKind;
use sc_core::ipc::StatusReport;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[cfg(target_os = "linux")]
type PipelineSlot = Option<crate::pipeline::Pipeline>;
#[cfg(not(target_os = "linux"))]
type PipelineSlot = Option<()>;

/// Shared daemon state. Config lives behind an RwLock so the hot-reload watcher
/// can swap it without disturbing readers; the capture pipeline lives behind a
/// Mutex so `sc save` and `sc status` can reach it from IPC threads.
#[derive(Clone)]
pub struct Daemon {
    pub config: Arc<RwLock<Config>>,
    status: Arc<Mutex<Status>>,
    pipeline: Arc<Mutex<PipelineSlot>>,
}

struct Status {
    state: CaptureState,
    recording: bool,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            status: Arc::new(Mutex::new(Status {
                state: CaptureState::Stopped,
                recording: false,
            })),
            pipeline: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_config(&self, config: Config) {
        *self.config.write().unwrap() = config;
    }
}

impl Daemon {
    /// Start the capture pipeline (pops the portal consent dialog on Linux).
    pub fn start_capture(&self) {
        #[cfg(target_os = "linux")]
        {
            let cfg = self.config.read().unwrap().clone();
            match crate::pipeline::Pipeline::start(&cfg) {
                Ok(p) => {
                    *self.pipeline.lock().unwrap() = Some(p);
                    self.set_state(CaptureState::Active);
                    tracing::info!("capture pipeline started");
                }
                Err(e) => tracing::error!(error = %e, "capture failed to start"),
            }
        }
        #[cfg(not(target_os = "linux"))]
        tracing::warn!("capture backend not implemented on this platform yet");
    }

    /// Write the last N seconds from the ring buffer to a file.
    pub fn save_clip(&self, last_secs: Option<u32>) -> Result<(PathBuf, f64), String> {
        #[cfg(target_os = "linux")]
        {
            let guard = self.pipeline.lock().unwrap();
            let p = guard.as_ref().ok_or("capture is not running")?;
            let cfg = self.config.read().unwrap();
            let secs = last_secs.unwrap_or(cfg.buffer.replay_duration_secs);
            let app = crate::naming::foreground_app();
            let path = crate::naming::clip_path(&cfg, app.as_deref());
            let dur = p.save(secs, &path).map_err(|e| e.to_string())?;
            crate::notify::clip_saved(&path, dur, &cfg);
            Ok((path, dur))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = last_secs;
            Err("capture backend not implemented on this platform yet".into())
        }
    }
}

impl Daemon {
    pub fn status_report(&self) -> StatusReport {
        let status = self.status.lock().unwrap();
        let cfg = self.config.read().unwrap();
        let (fill, encoder, monitor) = self.capture_info();
        StatusReport {
            state: status.state,
            recording: status.recording,
            buffer_secs: cfg.buffer.replay_duration_secs,
            buffer_fill: fill,
            monitor,
            encoder,
            drift_ms: 0.0,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    fn capture_info(
        &self,
    ) -> (
        f32,
        Option<EncoderKind>,
        Option<sc_core::capture::MonitorInfo>,
    ) {
        #[cfg(target_os = "linux")]
        if let Some(p) = self.pipeline.lock().unwrap().as_ref() {
            let mon = sc_core::capture::MonitorInfo {
                id: "captured".into(),
                name: "captured display".into(),
                width: p.width,
                height: p.height,
                refresh_mhz: 0,
                primary: true,
            };
            return (p.buffer_fill(), Some(EncoderKind::Vaapi), Some(mon));
        }
        (0.0, None, None)
    }

    pub fn set_state(&self, state: CaptureState) {
        self.status.lock().unwrap().state = state;
    }

    pub fn set_recording(&self, recording: bool) {
        self.status.lock().unwrap().recording = recording;
    }
}

impl Daemon {
    /// Invoked by the in-app hotkey listener.
    pub fn hotkey_save(&self) {
        match self.save_clip(None) {
            Ok((path, secs)) => {
                tracing::info!(path = %path.display(), secs, "clip saved via hotkey")
            }
            Err(e) => tracing::warn!(error = %e, "hotkey save failed"),
        }
    }
}
