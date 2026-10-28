#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio_mix;
#[cfg(windows)]
mod discord;
#[cfg(windows)]
mod games;
#[cfg(target_os = "linux")]
mod hypr;
mod naming;
mod notify;
#[cfg(any(target_os = "linux", windows))]
mod pipeline;
#[cfg(windows)]
mod popup;
mod reload;
mod server;
mod state;
mod thumbs;
#[cfg(windows)]
mod win;

use anyhow::{Context, Result};
use clap::Parser;
use sc_core::config::Config;
use state::Daemon;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "scd",
    about = "SimpleClip daemon: owns capture, buffer, encode, save"
)]
struct Args {
    #[arg(long)]
    foreground: bool,
    #[arg(long, short)]
    verbose: bool,
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let _log_guard = init_logging(args.verbose, args.foreground);

    #[cfg(windows)]
    win::init_dpi();
    #[cfg(windows)]
    if !win::acquire_single_instance() {
        tracing::warn!("scd is already running; exiting");
        return Ok(());
    }

    let config_path = match args.config {
        Some(p) => p,
        None => Config::default_path().context("resolving config path")?,
    };
    let config = load_or_create(&config_path)?;

    #[cfg(windows)]
    popup::configure(&config.general);
    let daemon = Daemon::new(config);
    reload::spawn_watcher(config_path, daemon.clone());
    std::thread::spawn(thumbs::prune);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "scd starting");

    {
        let d = daemon.clone();
        std::thread::spawn(move || {
            #[cfg(windows)]
            {
                let mut wait = 5;
                while !d.start_capture() {
                    std::thread::sleep(std::time::Duration::from_secs(wait));
                    wait = (wait * 2).min(60);
                }
            }
            #[cfg(not(windows))]
            d.start_capture();
        });
    }

    #[cfg(target_os = "linux")]
    hypr::sync_hotkey(&daemon.config.read().unwrap().hotkeys.save);

    #[cfg(windows)]
    {
        win::set_autostart(daemon.config.read().unwrap().general.autostart);
        win::spawn_hotkey(daemon.clone());
        discord::spawn(daemon.clone());
        let server_daemon = daemon.clone();
        std::thread::Builder::new()
            .name("sc-ipc".into())
            .spawn(move || {
                if let Err(e) = server::serve(server_daemon) {
                    tracing::error!(error = format!("{e:#}"), "IPC server failed");
                    std::process::exit(1);
                }
            })
            .context("ipc thread")?;
        win::announce_startup(daemon.clone());
        win::run_tray(daemon);
        tracing::info!("scd quitting");
        Ok(())
    }
    #[cfg(not(windows))]
    server::serve(daemon)
}

fn load_or_create(path: &std::path::Path) -> Result<Config> {
    Ok(Config::load_or_recover(path))
}

#[cfg(not(windows))]
fn init_logging(verbose: bool, _foreground: bool) -> Option<()> {
    use tracing_subscriber::EnvFilter;
    let level = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env("SC_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    None
}

#[cfg(windows)]
fn prune_logs(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("scd.log"))
        })
        .collect();
    logs.sort();
    let drop = logs.len().saturating_sub(keep);
    for p in logs.into_iter().take(drop) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(windows)]
fn init_logging(
    verbose: bool,
    foreground: bool,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    let level = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env("SC_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false);

    let console = if foreground {
        win::attach_parent_console()
    } else {
        None
    };
    let guard = match console {
        Some(console) => {
            builder.with_writer(std::sync::Mutex::new(console)).init();
            None
        }
        None => {
            let dir = directories::ProjectDirs::from("", "", "SimpleClip")
                .map(|d| d.data_local_dir().join("logs"))
                .unwrap_or_else(|| PathBuf::from("."));
            prune_logs(&dir, 7);
            let appender = tracing_appender::rolling::daily(dir, "scd.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            builder.with_writer(writer).init();
            Some(guard)
        }
    };

    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));
    guard
}
