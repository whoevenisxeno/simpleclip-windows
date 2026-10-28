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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PopupStyle {
    #[default]
    Detailed,
    Minimal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PopupPosition {
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    Center,
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
    Mixed,
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
    pub desktop_volume: u32,
    pub mic_volume: u32,
}

pub const MAX_VOLUME_PERCENT: u32 = 200;

impl AudioSettings {
    pub fn same_capture_as(&self, other: &Self) -> bool {
        let mut a = self.clone();
        a.desktop_volume = other.desktop_volume;
        a.mic_volume = other.mic_volume;
        a == *other
    }
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
    pub gpu_convert: bool,
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SaveSettings {
    pub directory: Option<PathBuf>,
    pub trim_directory: Option<PathBuf>,
    pub filename_template: String,
    pub folder_policy: FolderPolicy,
    pub container: Container,
    pub warn_at_gb: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct HotkeySettings {
    pub save: String,
    pub screenshot: String,
    pub open: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GeneralSettings {
    pub update_check: bool,
    pub save_sound: bool,
    pub update_sound: bool,
    pub save_sound_file: Option<PathBuf>,
    pub popup_style: PopupStyle,
    pub popup_position: PopupPosition,
    pub popup_animation: bool,
    pub star_prompt_done: bool,
    pub star_prompt_next: Option<u64>,
    pub settings_version: Option<u32>,
    pub last_version: Option<String>,
    pub theme: String,
    pub accent: Option<String>,
    pub share_prompt: bool,
    pub discord_presence: bool,
    pub notify: bool,
    pub post_save_hook: Option<PathBuf>,
    pub autostart: bool,
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
            mic_enabled: cfg!(windows),
            tracks: AudioTracks::Mixed,
            desktop_volume: 100,
            mic_volume: 100,
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
            gpu_convert: true,
            max_height: None,
        }
    }
}

impl Default for SaveSettings {
    fn default() -> Self {
        Self {
            directory: None,
            trim_directory: None,
            filename_template: "sc-{app}-{date}".to_string(),
            folder_policy: FolderPolicy::PerDay,
            container: Container::Mp4,
            warn_at_gb: 50,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        if cfg!(windows) {
            Self {
                save: "ALT+F10".to_string(),
                screenshot: "ALT+F9".to_string(),
                open: "ALT+F8".to_string(),
            }
        } else {
            Self {
                save: "SUPER+F10".to_string(),
                screenshot: "SUPER+F9".to_string(),
                open: "SUPER+F8".to_string(),
            }
        }
    }
}

impl SaveSettings {
    pub fn resolve_dir(&self) -> PathBuf {
        if let Some(dir) = &self.directory {
            return dir.clone();
        }
        let videos = directories::UserDirs::new().and_then(|d| d.video_dir().map(PathBuf::from));
        match videos {
            Some(v) if cfg!(windows) => v.join("SimpleClip"),
            Some(v) => v,
            None => PathBuf::from("."),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            update_check: true,
            save_sound: true,
            update_sound: true,
            save_sound_file: None,
            popup_style: PopupStyle::Detailed,
            popup_position: PopupPosition::TopRight,
            popup_animation: true,
            star_prompt_done: false,
            star_prompt_next: None,
            settings_version: None,
            last_version: None,
            theme: "system".to_string(),
            accent: None,
            share_prompt: true,
            discord_presence: true,
            notify: true,
            post_save_hook: None,
            autostart: true,
            setup_complete: false,
        }
    }
}

const MAX_REPLAY_SECS: u32 = 300;

impl Config {
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
        let mut cfg: Config = toml::from_str(&text).map_err(|e| Error::Config(e.to_string()))?;
        if cfg.general.settings_version.is_none() {
            cfg.encode.gpu_convert = true;
            cfg.general.settings_version = Some(1);
            let _ = cfg.save(path);
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load_or_recover(path: &std::path::Path) -> Self {
        match Self::load(path) {
            Ok(cfg) => cfg,
            Err(Error::ConfigMissing(_)) => {
                let cfg = Self::default();
                let _ = cfg.save(path);
                cfg
            }
            Err(e) => {
                tracing::warn!(error = %e, "config unusable, starting from defaults");
                let _ = std::fs::rename(path, path.with_extension("toml.bad"));
                let cfg = Self::default();
                let _ = cfg.save(path);
                cfg
            }
        }
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
        if let Some(h) = self.encode.max_height {
            if !(240..=4320).contains(&h) {
                return Err(Error::Config("encode.max-height must be 240..=4320".into()));
            }
        }
        if self.capture.target_fps == 0 || self.capture.target_fps > 240 {
            return Err(Error::Config("capture.target-fps must be 1..=240".into()));
        }
        if self.audio.desktop_volume > MAX_VOLUME_PERCENT
            || self.audio.mic_volume > MAX_VOLUME_PERCENT
        {
            return Err(Error::Config(format!(
                "audio volumes must be 0..={MAX_VOLUME_PERCENT}"
            )));
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
    fn volumes_default_to_unchanged_and_are_capped() {
        let mut cfg = Config::default();
        assert_eq!((cfg.audio.desktop_volume, cfg.audio.mic_volume), (100, 100));
        cfg.audio.mic_volume = MAX_VOLUME_PERCENT + 1;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn volume_change_is_not_a_capture_change() {
        let a = AudioSettings::default();
        let mut b = a.clone();
        b.mic_volume = 40;
        assert!(a.same_capture_as(&b));
        b.mic_enabled = !b.mic_enabled;
        assert!(!a.same_capture_as(&b));
    }

    #[test]
    fn old_config_without_new_fields_still_loads() {
        let cfg: Config =
            toml::from_str("[hotkeys]\nsave = \"CTRL+G\"\n[audio]\nmic-enabled = false\n").unwrap();
        assert_eq!(cfg.hotkeys.save, "CTRL+G");
        assert_eq!(cfg.audio.mic_volume, 100);
        assert!(!cfg.hotkeys.open.is_empty());
    }

    #[test]
    fn rejects_buffer_over_ram_cap() {
        let mut cfg = Config::default();
        cfg.buffer.ram_cap_mb = 1;
        cfg.encode.bitrate_kbps = 50_000;
        assert!(cfg.validate().is_err());
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn broken_config_is_moved_aside_and_replaced() {
        let dir = std::env::temp_dir().join(format!("sc-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is = [not valid toml").unwrap();
        let cfg = Config::load_or_recover(&path);
        assert_eq!(cfg.buffer.replay_duration_secs, 30);
        assert!(dir.join("config.toml.bad").exists());
        assert!(Config::load(&path).is_ok(), "defaults were written back");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
