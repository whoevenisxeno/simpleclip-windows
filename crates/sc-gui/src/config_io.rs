use sc_core::config::Config;
use std::path::PathBuf;

/// The GUI reads and writes the same TOML the daemon hot-reloads; saving from
/// the wizard or settings triggers the daemon's reload path. No config is
/// tunneled through IPC.
pub fn path() -> PathBuf {
    Config::default_path().unwrap_or_else(|_| PathBuf::from("simpleclip-config.toml"))
}

pub fn load() -> Config {
    Config::load(&path()).unwrap_or_default()
}

pub fn save(cfg: &Config) -> Result<(), String> {
    cfg.validate().map_err(|e| e.to_string())?;
    cfg.save(&path()).map_err(|e| e.to_string())
}
