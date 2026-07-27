use crate::encode::Codec;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Container {
    Mp4,
    Mkv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FolderPolicy {
    Flat,
    PerDay,
    PerApp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioTracks {
    /// Single mixed track (default, most compatible).
    Mixed,
    /// Desktop on track 1, mic on track 2 (best in MKV, good for editing).
    Separate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct CaptureSettings {
    pub monitor_id: String,
    pub show_cursor: bool,
    pub target_fps: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AudioSettings {
    pub desktop_enabled: bool,
    pub desktop_device: String,
    pub mic_device: Option<String>,
    pub mic_enabled: bool,
    pub tracks: AudioTracks,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BufferSettings {
    pub replay_duration_secs: u32,
    pub ram_cap_mb: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EncodeSettings {
    pub codec: Codec,
    pub bitrate_kbps: u32,
    pub gop_frames: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SaveSettings {
    pub directory: Option<PathBuf>,
    pub filename_template: String,
    pub folder_policy: FolderPolicy,
    pub container: Container,
    pub warn_at_gb: u32,
}

/// Windows uses these directly (RegisterHotKey). On Linux they are advisory:
/// the wizard teaches a compositor bind to `sc save` instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct HotkeySettings {
    pub save: String,
    pub screenshot: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GeneralSettings {
    pub update_check: bool,
    pub save_sound: bool,
    pub notify: bool,
    pub post_save_hook: Option<PathBuf>,
    /// Set once the first-launch wizard finishes, so it never runs again.
    pub setup_complete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub capture: CaptureSettings,
    pub audio: AudioSettings,
    pub buffer: BufferSettings,
    pub encode: EncodeSettings,
    pub save: SaveSettings,
    pub hotkeys: HotkeySettings,
    pub general: GeneralSettings,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            monitor_id: String::new(),
            show_cursor: true,
            target_fps: 60,
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            desktop_enabled: true,
            desktop_device: String::new(),
            mic_device: None,
            mic_enabled: false,
            tracks: AudioTracks::Mixed,
        }
    }
}

impl Default for BufferSettings {
    fn default() -> Self {
        Self {
            replay_duration_secs: 30,
            ram_cap_mb: 2048,
        }
    }
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            codec: Codec::H264,
            bitrate_kbps: 8_000,
            gop_frames: 120,
        }
    }
}

impl Default for SaveSettings {
    fn default() -> Self {
        Self {
            directory: None,
            filename_template: "sc-{app}-{date}".to_string(),
            folder_policy: FolderPolicy::PerDay,
            container: Container::Mp4,
            warn_at_gb: 50,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            save: "SUPER+F10".to_string(),
            screenshot: "SUPER+F9".to_string(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            update_check: true,
            save_sound: true,
            notify: true,
            post_save_hook: None,
            setup_complete: false,
        }
    }
}

const MAX_REPLAY_SECS: u32 = 300; // hard cap: 5 minutes (§6.2)

impl Config {
    /// `~/.config/simpleclip/config.toml` (XDG) or `%APPDATA%\SimpleClip\config.toml`.
    pub fn default_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "SimpleClip")
            .ok_or_else(|| Error::Config("cannot resolve config directory".into()))?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Err(Error::ConfigMissing(path.to_path_buf()));
        }
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text).map_err(|e| Error::Config(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

impl Config {
    /// Rough RAM the ring buffer will occupy: duration x bitrate across tracks.
    /// The wizard shows this and refuses to exceed `buffer.ram_cap_mb` rather
    /// than silently degrading quality (§6.2).
    pub fn estimated_buffer_mb(&self) -> u32 {
        let audio_kbps = if self.audio.mic_enabled { 320 } else { 160 };
        let total_kbps = self.encode.bitrate_kbps + audio_kbps;
        let bytes = (total_kbps as u64 * 1000 / 8) * self.buffer.replay_duration_secs as u64;
        (bytes / (1024 * 1024)) as u32
    }

    pub fn validate(&self) -> Result<()> {
        if self.buffer.replay_duration_secs == 0 {
            return Err(Error::Config(
                "buffer.replay-duration-secs must be > 0".into(),
            ));
        }
        if self.buffer.replay_duration_secs > MAX_REPLAY_SECS {
            return Err(Error::Config(format!(
                "buffer.replay-duration-secs exceeds hard cap of {MAX_REPLAY_SECS}s"
            )));
        }
        if self.capture.target_fps == 0 || self.capture.target_fps > 240 {
            return Err(Error::Config("capture.target-fps must be 1..=240".into()));
        }
        let est = self.estimated_buffer_mb();
        if est > self.buffer.ram_cap_mb {
            return Err(Error::Config(format!(
                "estimated buffer {est} MB exceeds ram-cap-mb {}",
                self.buffer.ram_cap_mb
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        assert!(Config::default().validate().is_ok());
    }

    #[test]
    fn roundtrips_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn rejects_over_cap_duration() {
        let mut cfg = Config::default();
        cfg.buffer.replay_duration_secs = MAX_REPLAY_SECS + 1;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn rejects_buffer_over_ram_cap() {
        let mut cfg = Config::default();
        cfg.buffer.ram_cap_mb = 1;
        cfg.encode.bitrate_kbps = 50_000;
        assert!(cfg.validate().is_err());
    }
}
