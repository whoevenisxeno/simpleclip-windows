use crate::state::Daemon;
use notify::{Event, RecursiveMode, Watcher};
use sc_core::config::Config;
use std::path::PathBuf;
use std::sync::mpsc;

/// Watch the config file and hot-reload on change. Invalid configs are logged
/// and ignored, keeping the daemon on the last-good config (§8.2) rather than
/// crashing. Runs on its own thread; never touches the capture pipeline.
pub fn spawn_watcher(path: PathBuf, daemon: Daemon) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "cannot create config watcher");
                return;
            }
        };
        let watch_dir = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::error!(error = %e, "cannot watch config dir");
            return;
        }
        for res in rx {
            handle_event(res, &path, &daemon);
        }
    });
}

fn handle_event(res: notify::Result<Event>, path: &std::path::Path, daemon: &Daemon) {
    let Ok(event) = res else { return };
    if !event.paths.iter().any(|p| p == path) {
        return;
    }
    if !event.kind.is_modify() && !event.kind.is_create() {
        return;
    }
    match Config::load(path) {
        Ok(cfg) => {
            daemon.set_config(cfg);
            tracing::info!("config hot-reloaded");
        }
        Err(e) => {
            tracing::warn!(error = %e, "invalid config ignored; keeping last-good");
        }
    }
}
