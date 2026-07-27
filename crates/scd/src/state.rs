use sc_core::capture::CaptureState;
use sc_core::config::Config;
use sc_core::ipc::StatusReport;
use std::sync::{Arc, Mutex, RwLock};

/// Shared daemon state. Config lives behind an RwLock so the hot-reload watcher
/// can swap it without disturbing readers. Runtime status lives behind a Mutex.
/// In Phase 0 the pipeline is stubbed; Phase 1+ fills these in.
#[derive(Clone)]
pub struct Daemon {
    pub config: Arc<RwLock<Config>>,
    status: Arc<Mutex<Status>>,
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
        }
    }

    pub fn set_config(&self, config: Config) {
        *self.config.write().unwrap() = config;
    }
}

impl Daemon {
    pub fn status_report(&self) -> StatusReport {
        let status = self.status.lock().unwrap();
        let cfg = self.config.read().unwrap();
        StatusReport {
            state: status.state,
            recording: status.recording,
            buffer_secs: cfg.buffer.replay_duration_secs,
            buffer_fill: 0.0,
            monitor: None,
            encoder: None,
            drift_ms: 0.0,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn set_state(&self, state: CaptureState) {
        self.status.lock().unwrap().state = state;
    }

    pub fn set_recording(&self, recording: bool) {
        self.status.lock().unwrap().recording = recording;
    }
}
