mod reload;
mod server;
mod state;

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
    init_logging(args.verbose);

    let config_path = match args.config {
        Some(p) => p,
        None => Config::default_path().context("resolving config path")?,
    };
    let config = load_or_create(&config_path)?;

    let daemon = Daemon::new(config);
    reload::spawn_watcher(config_path, daemon.clone());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "scd starting");
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

fn init_logging(verbose: bool) {
    use tracing_subscriber::EnvFilter;
    let level = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env("SC_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
