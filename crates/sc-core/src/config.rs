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

/// How much the save card shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PopupStyle {
    /// File name, game, size, resolution, encoder and how long it took.
    #[default]
    Detailed,
    /// One short line.
    Minimal,
}

/// Where on the screen the cards appear.
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
    /// Loudness of game and app sound in clips, in percent (100 = unchanged).
    pub desktop_volume: u32,
    /// Loudness of the microphone in clips, in percent (100 = unchanged).
    pub mic_volume: u32,
}

/// The loudest a volume slider may go, in percent.
pub const MAX_VOLUME_PERCENT: u32 = 200;

impl AudioSettings {
    /// Same capture setup, ignoring the two volumes. A volume change applies to the
    /// running recorder on the fly; anything else here needs it restarted.
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
    /// Let the GPU encoder do the BGRA to YUV colour conversion instead of the CPU
    /// (Windows, NVENC/AMF/QuickSync). Lowers CPU use; falls back automatically.
    pub gpu_convert: bool,
    /// Scale the picture down so it is at most this many pixels tall (`None` keeps the
    /// screen's own size). Smaller pictures mean smaller files and less work.
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SaveSettings {
    pub directory: Option<PathBuf>,
    /// Where trimmed copies go. `None` puts each next to its original.
    pub trim_directory: Option<PathBuf>,
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
    /// Brings the SimpleClip window up (empty turns it off).
    pub open: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GeneralSettings {
    pub update_check: bool,
    pub save_sound: bool,
    /// Sound file to play on save (`None` plays the built-in one).
    pub save_sound_file: Option<PathBuf>,
    /// How much the save card shows.
    pub popup_style: PopupStyle,
    /// Where the cards appear.
    pub popup_position: PopupPosition,
    /// Slide the cards in and out instead of just showing them.
    pub popup_animation: bool,
    /// The user opened the GitHub page from the star reminder (or asked not to be asked
    /// again), so it never shows again.
    pub star_prompt_done: bool,
    /// Earliest time (seconds since 1970) the star reminder may show next.
    pub star_prompt_next: Option<u64>,
    /// Which settings migrations have run (so each runs only once on an old file).
    pub settings_version: Option<u32>,
    /// The app version that last ran, so an update can say "Updated to ...".
    pub last_version: Option<String>,
    /// Colour theme id ("system" follows Windows light/dark; the app knows the others).
    pub theme: String,
    /// Optional accent colour as `#rrggbb`, drawn over the theme's own.
    pub accent: Option<String>,
    /// After saving a trim, offer to copy the clip or compress it for Discord.
    pub share_prompt: bool,
    pub notify: bool,
    pub post_save_hook: Option<PathBuf>,
    /// Start the daemon at sign-in (Windows). On by default; the daemon keeps the
    /// OS startup entry in step with this.
    pub autostart: bool,
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
            // On by default on Windows, where mic capture is implemented; the
            // wizard and Settings let you turn it off.
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
        // Windows reserves most Win+key combos for the shell, so default to Alt.
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
    /// Where clips are written: the configured directory, else the user's Videos
    /// folder (in a `SimpleClip` subfolder on Windows, so clips don't mix with
    /// everything else living in Videos).
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
            notify: true,
            post_save_hook: None,
            autostart: true,
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
        let mut cfg: Config = toml::from_str(&text).map_err(|e| Error::Config(e.to_string()))?;
        if cfg.general.settings_version.is_none() {
            // 1: "Lower CPU use" became the default. Applied once to older files.
            cfg.encode.gpu_convert = true;
            cfg.general.settings_version = Some(1);
            let _ = cfg.save(path);
        }
        cfg.validate()?;
        Ok(cfg)
    }

    /// Load the config, or start fresh if it can not be used. A missing file is
    /// created with the defaults. A file that will not parse or fails validation
    /// is moved aside as `config.toml.bad` (never deleted) and replaced with the
    /// defaults, so a damaged file can not stop SimpleClip from starting.
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
