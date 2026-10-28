use sc_core::config::Config;
use std::path::PathBuf;

pub fn path() -> PathBuf {
    Config::default_path().unwrap_or_else(|_| PathBuf::from("simpleclip-config.toml"))
}

pub fn load() -> Config {
    Config::load_or_recover(&path())
}

pub fn save(cfg: &Config) -> Result<(), String> {
    cfg.validate().map_err(|e| e.to_string())?;
    cfg.save(&path()).map_err(|e| e.to_string())
}
