// Release builds on Windows are GUI-subsystem apps so the daemon starts at login
// without flashing a console window; logs go to a file (or to the parent
// terminal with --foreground).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio_mix;
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
    /// Run in the foreground (do not detach); logs also go to stderr.
    #[arg(long)]
    foreground: bool,
    /// Verbose (debug) logging.
    #[arg(long, short)]
    verbose: bool,
    /// Config file path (defaults to the platform config dir).
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

    let daemon = Daemon::new(config);
    reload::spawn_watcher(config_path, daemon.clone());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "scd starting");

    // Start capture off-thread so the IPC server is reachable immediately (the
    // portal consent dialog can take a while to be answered).
    {
        let d = daemon.clone();
        std::thread::spawn(move || {
            // On Windows a failure is usually transient (no display yet at
            // login, a GPU driver restart), so keep trying instead of giving up.
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

    // Keep the compositor's save-hotkey bind in sync with config (no elevated
    // permissions needed; SimpleClip owns and reloads its own Hyprland snippet).
    #[cfg(target_os = "linux")]
    hypr::sync_hotkey(&daemon.config.read().unwrap().hotkeys.save);

    #[cfg(windows)]
    {
        // Keep the Windows startup entry in step with the setting (on by default),
        // so the replay buffer is always running in the background after sign-in.
        win::set_autostart(daemon.config.read().unwrap().general.autostart);
        win::spawn_hotkey(daemon.clone());
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
        // The tray owns the main thread's message loop until the user quits.
        win::run_tray(daemon);
        tracing::info!("scd quitting");
        Ok(())
    }
    #[cfg(not(windows))]
    server::serve(daemon)
}

fn load_or_create(path: &std::path::Path) -> Result<Config> {
    match Config::load(path) {
        Ok(cfg) => Ok(cfg),
        Err(sc_core::Error::ConfigMissing(_)) => {
            let cfg = Config::default();
            cfg.save(path).context("writing default config")?;
            tracing::info!(path = %path.display(), "wrote default config");
            Ok(cfg)
        }
        Err(e) => Err(e).context("loading config"),
    }
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

/// Windows logging: to the parent terminal with `--foreground`, otherwise to a
/// rolling file under `%LOCALAPPDATA%\SimpleClip\logs`. The returned guard must
/// outlive `main` so buffered lines are flushed.
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
            let appender = tracing_appender::rolling::daily(dir, "scd.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            builder.with_writer(writer).init();
            Some(guard)
        }
    };

    // A windowless process otherwise dies silently; leave a trace in the log.
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));
    guard
}
