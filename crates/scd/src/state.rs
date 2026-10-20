use sc_core::capture::CaptureState;
use sc_core::config::Config;
use sc_core::encode::EncoderKind;
use sc_core::ipc::StatusReport;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[cfg(any(target_os = "linux", windows))]
type PipelineSlot = Option<crate::pipeline::Pipeline>;
#[cfg(not(any(target_os = "linux", windows)))]
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
        #[cfg(windows)]
        crate::popup::configure(&config.general);
        *self.config.write().unwrap() = config;
    }

    /// Swap in a reloaded config. Settings the live pipeline was built from
    /// (capture, audio, buffer, encode) need a pipeline restart to take effect;
    /// everything else (hotkeys, naming, notifications) is read on use.
    pub fn apply_config(&self, new: Config) {
        let needs_restart = {
            let old = self.config.read().unwrap();
            old.capture != new.capture
                || old.audio != new.audio
                || old.buffer != new.buffer
                || old.encode != new.encode
        };
        #[cfg(windows)]
        crate::win::set_autostart(new.general.autostart);
        self.set_config(new);
        if needs_restart && self.pipeline.lock().unwrap().is_some() {
            tracing::info!("capture settings changed; restarting pipeline");
            let d = self.clone();
            std::thread::spawn(move || {
                d.stop_capture();
                d.start_capture();
            });
        }
    }
}

impl Daemon {
    /// Start the capture pipeline (pops the portal consent dialog on Linux).
    /// Returns whether capture is now running.
    pub fn start_capture(&self) -> bool {
        #[cfg(any(target_os = "linux", windows))]
        {
            let cfg = self.config.read().unwrap().clone();
            match crate::pipeline::Pipeline::start(&cfg) {
                Ok(p) => {
                    *self.pipeline.lock().unwrap() = Some(p);
                    self.set_state(CaptureState::Active);
                    tracing::info!("capture pipeline started");
                    true
                }
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"), "capture failed to start");
                    false
                }
            }
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            tracing::warn!("capture backend not implemented on this platform yet");
            false
        }
    }

    /// Tear the pipeline down (drops capture + encoders and frees the buffer).
    pub fn stop_capture(&self) {
        *self.pipeline.lock().unwrap() = None;
        self.set_state(CaptureState::Stopped);
    }

    /// Write the last N seconds from the ring buffer to a file.
    pub fn save_clip(&self, last_secs: Option<u32>) -> Result<(PathBuf, f64), String> {
        #[cfg(any(target_os = "linux", windows))]
        {
            let guard = self.pipeline.lock().unwrap();
            let p = guard.as_ref().ok_or("capture is not running")?;
            let cfg = self.config.read().unwrap();
            let secs = last_secs.unwrap_or(cfg.buffer.replay_duration_secs);
            let app = crate::naming::foreground_app();
            let path = crate::naming::clip_path(&cfg, app.as_deref());
            let started = std::time::Instant::now();
            let dur = p.save(secs, &path).map_err(|e| format!("{e:#}"))?;
            #[cfg(windows)]
            crate::notify::clip_saved(
                &crate::notify::SavedClip {
                    path: &path,
                    secs: dur,
                    app: app.as_deref(),
                    encoder: p.encoder,
                    width: p.width,
                    height: p.height,
                    fps: cfg.capture.target_fps,
                    took: started.elapsed(),
                },
                &cfg,
            );
            #[cfg(not(windows))]
            {
                let _ = started;
                crate::notify::clip_saved(&path, dur, &cfg);
            }
            Ok((path, dur))
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = last_secs;
            Err("capture backend not implemented on this platform yet".into())
        }
    }

    /// Save the current frame as a PNG next to the clips.
    pub fn screenshot(&self) -> Result<PathBuf, String> {
        #[cfg(windows)]
        {
            let guard = self.pipeline.lock().unwrap();
            let p = guard.as_ref().ok_or("capture is not running")?;
            let cfg = self.config.read().unwrap();
            let app = crate::naming::foreground_app();
            let path = crate::naming::screenshot_path(&cfg, app.as_deref());
            p.screenshot(&path).map_err(|e| format!("{e:#}"))?;
            crate::notify::screenshot_saved(&path, p.width, p.height, &cfg);
            Ok(path)
        }
        #[cfg(not(windows))]
        Err("screenshots aren't supported on this platform yet".into())
    }

    /// Pause or resume feeding the replay buffer. Capture itself keeps running so
    /// resuming is instant.
    pub fn set_paused(&self, paused: bool) {
        #[cfg(any(target_os = "linux", windows))]
        {
            // Take the pipeline lock and release it before touching `status`:
            // status_report locks them in the opposite order.
            let applied = match self.pipeline.lock().unwrap().as_ref() {
                Some(p) => {
                    p.set_paused(paused);
                    true
                }
                None => false,
            };
            if applied {
                self.set_state(if paused {
                    CaptureState::Paused
                } else {
                    CaptureState::Active
                });
            }
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = paused;
    }

    pub fn capture_state(&self) -> CaptureState {
        self.status.lock().unwrap().state
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
        #[cfg(any(target_os = "linux", windows))]
        if let Some(p) = self.pipeline.lock().unwrap().as_ref() {
            let mon = sc_core::capture::MonitorInfo {
                id: "captured".into(),
                name: "captured display".into(),
                width: p.width,
                height: p.height,
                refresh_mhz: 0,
                primary: true,
            };
            return (p.buffer_fill(), Some(p.encoder), Some(mon));
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
