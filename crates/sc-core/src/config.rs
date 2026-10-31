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
    pub sync_offset_ms: i32,
}

pub const MAX_VOLUME_PERCENT: u32 = 200;
pub const MAX_SYNC_OFFSET_MS: i32 = 2000;

impl AudioSettings {
    pub fn same_capture_as(&self, other: &Self) -> bool {
        let mut a = self.clone();
        a.desktop_volume = other.desktop_volume;
        a.mic_volume = other.mic_volume;
        a.sync_offset_ms = other.sync_offset_ms;
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
    pub extra: Vec<ExtraHotkey>,
}

pub const MAX_EXTRA_HOTKEYS: usize = 12;
pub const SAVE_LAST_CHOICES: [u32; 6] = [10, 15, 30, 60, 120, 300];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyAction {
    SaveLast,
    Screenshot,
    TogglePause,
    OpenApp,
    OpenFolder,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 5] = [
        HotkeyAction::SaveLast,
        HotkeyAction::Screenshot,
        HotkeyAction::TogglePause,
        HotkeyAction::OpenApp,
        HotkeyAction::OpenFolder,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::SaveLast => "Save the last…",
            HotkeyAction::Screenshot => "Take a screenshot",
            HotkeyAction::TogglePause => "Pause or resume recording",
            HotkeyAction::OpenApp => "Open SimpleClip",
            HotkeyAction::OpenFolder => "Open the clips folder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ExtraHotkey {
    pub keys: String,
    pub action: HotkeyAction,
    pub secs: u32,
}

impl Default for ExtraHotkey {
    fn default() -> Self {
        Self {
            keys: String::new(),
            action: HotkeyAction::SaveLast,
            secs: 30,
        }
    }
}

impl ExtraHotkey {
    pub fn describe(&self) -> String {
        match self.action {
            HotkeyAction::SaveLast => format!("Save the last {}", describe_secs(self.secs)),
            other => other.label().to_string(),
        }
    }
}

pub fn describe_secs(secs: u32) -> String {
    if secs >= 60 && secs.is_multiple_of(60) {
        let m = secs / 60;
        format!("{m} minute{}", if m == 1 { "" } else { "s" })
    } else {
        format!("{secs} seconds")
    }
}

impl HotkeySettings {
    pub fn duplicate_of(&self, index: usize) -> Option<String> {
        let keys = self.extra.get(index)?.keys.trim().to_ascii_uppercase();
        if keys.is_empty() {
            return None;
        }
        let same = |other: &str| other.trim().to_ascii_uppercase() == keys;
        if same(&self.save) {
            return Some("Save clip".to_string());
        }
        if same(&self.screenshot) {
            return Some("Take screenshot".to_string());
        }
        if same(&self.open) {
            return Some("Open SimpleClip".to_string());
        }
        self.extra
            .iter()
            .take(index)
            .find(|e| same(&e.keys))
            .map(|e| e.describe())
    }
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
    pub events: EventsSettings,
    pub addons: AddonSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AddonSettings {
    pub auto_update: bool,
}

impl Default for AddonSettings {
    fn default() -> Self {
        Self { auto_update: true }
    }
}

pub const EVENT_SAVE_CHOICES: [u32; 5] = [10, 15, 20, 30, 60];
pub const EVENT_MAX_KILLS: u32 = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EventsSettings {
    pub enabled: bool,
    pub cs2: bool,
    pub league: bool,
    pub r6: bool,
    pub dota2: bool,
    pub min_kills: u32,
    pub save_secs: u32,
    pub test_mode: bool,
}

impl Default for EventsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            cs2: true,
            league: true,
            r6: true,
            dota2: true,
            min_kills: 2,
            save_secs: 20,
            test_mode: false,
        }
    }
}

impl EventsSettings {
    pub fn validate(&self) -> Result<()> {
        if !(1..=EVENT_MAX_KILLS).contains(&self.min_kills) {
            return Err(Error::Config(format!(
                "events.min-kills must be 1..={EVENT_MAX_KILLS}"
            )));
        }
        if !(5..=MAX_REPLAY_SECS).contains(&self.save_secs) {
            return Err(Error::Config(format!(
                "events.save-secs must be 5..={MAX_REPLAY_SECS}"
            )));
        }
        Ok(())
    }
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
            sync_offset_ms: 0,
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
                extra: Vec::new(),
            }
        } else {
            Self {
                save: "SUPER+F10".to_string(),
                screenshot: "SUPER+F9".to_string(),
                open: "SUPER+F8".to_string(),
                extra: Vec::new(),
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
        if self.audio.sync_offset_ms.abs() > MAX_SYNC_OFFSET_MS {
            return Err(Error::Config(format!(
                "audio.sync-offset-ms must be within {MAX_SYNC_OFFSET_MS}"
            )));
        }
        if self.hotkeys.extra.len() > MAX_EXTRA_HOTKEYS {
            return Err(Error::Config(format!(
                "hotkeys.extra can have at most {MAX_EXTRA_HOTKEYS} shortcuts"
            )));
        }
        if self
            .hotkeys
            .extra
            .iter()
            .any(|e| e.action == HotkeyAction::SaveLast && !(1..=MAX_REPLAY_SECS).contains(&e.secs))
        {
            return Err(Error::Config(format!(
                "hotkeys.extra secs must be 1..={MAX_REPLAY_SECS}"
            )));
        }
        self.events.validate()?;
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

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod extra_hotkey_tests {
    use super::*;

    fn extra(keys: &str, action: HotkeyAction, secs: u32) -> ExtraHotkey {
        ExtraHotkey {
            keys: keys.into(),
            action,
            secs,
        }
    }

    #[test]
    fn a_config_without_extra_shortcuts_has_none() {
        let cfg: Config = toml::from_str("[hotkeys]\nsave = \"CTRL+G\"\n").unwrap();
        assert!(cfg.hotkeys.extra.is_empty());
    }

    #[test]
    fn extra_shortcuts_survive_a_round_trip() {
        let mut cfg = Config::default();
        cfg.hotkeys.extra = vec![
            extra("CTRL+F1", HotkeyAction::SaveLast, 120),
            extra("CTRL+F2", HotkeyAction::TogglePause, 30),
            extra("", HotkeyAction::OpenFolder, 30),
        ];
        let text = toml::to_string(&cfg).unwrap();
        assert!(text.contains("save-last"), "{text}");
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.hotkeys.extra, cfg.hotkeys.extra);
    }

    #[test]
    fn a_shortcut_missing_its_length_gets_thirty_seconds() {
        let cfg: Config =
            toml::from_str("[[hotkeys.extra]]\nkeys = \"F5\"\naction = \"save-last\"\n").unwrap();
        assert_eq!(cfg.hotkeys.extra[0].secs, 30);
    }

    #[test]
    fn silly_lengths_and_too_many_shortcuts_are_refused() {
        let mut cfg = Config::default();
        cfg.hotkeys.extra = vec![extra("F5", HotkeyAction::SaveLast, 0)];
        assert!(cfg.validate().is_err());
        cfg.hotkeys.extra = vec![extra("F5", HotkeyAction::SaveLast, 301)];
        assert!(cfg.validate().is_err());
        cfg.hotkeys.extra = vec![extra("F5", HotkeyAction::SaveLast, 300)];
        assert!(cfg.validate().is_ok());
        cfg.hotkeys.extra = (0..=MAX_EXTRA_HOTKEYS)
            .map(|i| extra(&format!("F{}", i + 1), HotkeyAction::Screenshot, 30))
            .collect();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn a_length_only_matters_for_saving() {
        let mut cfg = Config::default();
        cfg.hotkeys.extra = vec![extra("F5", HotkeyAction::Screenshot, 0)];
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn descriptions_read_naturally() {
        assert_eq!(
            extra("F5", HotkeyAction::SaveLast, 15).describe(),
            "Save the last 15 seconds"
        );
        assert_eq!(
            extra("F5", HotkeyAction::SaveLast, 120).describe(),
            "Save the last 2 minutes"
        );
        assert_eq!(
            extra("F5", HotkeyAction::SaveLast, 60).describe(),
            "Save the last 1 minute"
        );
        assert_eq!(
            extra("F5", HotkeyAction::TogglePause, 30).describe(),
            "Pause or resume recording"
        );
    }

    #[test]
    fn duplicates_name_what_they_clash_with() {
        let mut h = HotkeySettings::default();
        h.extra = vec![
            extra("alt+f10", HotkeyAction::Screenshot, 30),
            extra("CTRL+F1", HotkeyAction::SaveLast, 60),
            extra("ctrl+f1", HotkeyAction::OpenApp, 30),
            extra("", HotkeyAction::OpenApp, 30),
            extra("CTRL+F9", HotkeyAction::OpenApp, 30),
        ];
        assert_eq!(h.duplicate_of(0).as_deref(), Some("Save clip"));
        assert_eq!(h.duplicate_of(1), None);
        assert_eq!(h.duplicate_of(2).as_deref(), Some("Save the last 1 minute"));
        assert_eq!(h.duplicate_of(3), None);
        assert_eq!(h.duplicate_of(4), None);
        assert_eq!(h.duplicate_of(9), None);
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod events_tests {
    use super::*;

    #[test]
    fn game_events_start_switched_off() {
        let cfg = Config::default();
        assert!(!cfg.events.enabled);
        assert_eq!(cfg.events.min_kills, 2);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn an_old_config_has_no_events_section() {
        let cfg: Config = toml::from_str("[hotkeys]\nsave = \"CTRL+G\"\n").unwrap();
        assert_eq!(cfg.events, EventsSettings::default());
    }

    #[test]
    fn the_events_section_round_trips() {
        let mut cfg = Config::default();
        cfg.events = EventsSettings {
            enabled: true,
            cs2: false,
            league: true,
            r6: false,
            dota2: true,
            min_kills: 3,
            save_secs: 30,
            test_mode: true,
        };
        let back: Config = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.events, cfg.events);
    }

    #[test]
    fn silly_event_settings_are_refused() {
        for (kills, secs) in [(0, 20), (6, 20), (2, 4), (2, 301)] {
            let events = EventsSettings {
                min_kills: kills,
                save_secs: secs,
                ..Default::default()
            };
            assert!(events.validate().is_err(), "{kills} {secs}");
        }
    }
}
