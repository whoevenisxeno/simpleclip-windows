//! Named recording setups. A preset holds how SimpleClip records (frame rate,
//! quality, sound, replay length) and nothing about this PC (displays, devices,
//! folders) or the look, so applying one can never break a working setup. Three
//! come built in; your own are saved next to the config file.

use sc_core::config::{AudioTracks, Config};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub fps: u32,
    pub cursor: bool,
    pub desktop_sound: bool,
    pub mic: bool,
    pub separate_tracks: bool,
    pub replay_secs: u32,
    pub bitrate_kbps: u32,
    /// Keyframe interval in seconds.
    pub keyframe_secs: u32,
    pub gpu_convert: bool,
    pub max_height: Option<u32>,
}

/// The presets that ship with SimpleClip (they can not be deleted).
pub fn built_in() -> Vec<Preset> {
    let base = Preset {
        name: String::new(),
        fps: 60,
        cursor: true,
        desktop_sound: true,
        mic: true,
        separate_tracks: false,
        replay_secs: 30,
        bitrate_kbps: 8_000,
        keyframe_secs: 2,
        gpu_convert: true,
        max_height: None,
    };
    vec![
        Preset {
            name: "Balanced".into(),
            ..base.clone()
        },
        Preset {
            name: "Light".into(),
            fps: 30,
            bitrate_kbps: 4_000,
            max_height: Some(720),
            ..base.clone()
        },
        Preset {
            name: "Sharp".into(),
            bitrate_kbps: 20_000,
            replay_secs: 60,
            ..base
        },
    ]
}

/// The current recording settings as a preset called `name`.
pub fn capture(cfg: &Config, name: &str) -> Preset {
    let fps = cfg.capture.target_fps.max(1);
    Preset {
        name: name.trim().to_string(),
        fps,
        cursor: cfg.capture.show_cursor,
        desktop_sound: cfg.audio.desktop_enabled,
        mic: cfg.audio.mic_enabled,
        separate_tracks: cfg.audio.tracks == AudioTracks::Separate,
        replay_secs: cfg.buffer.replay_duration_secs,
        bitrate_kbps: cfg.encode.bitrate_kbps,
        keyframe_secs: ((cfg.encode.gop_frames + fps / 2) / fps).max(1),
        gpu_convert: cfg.encode.gpu_convert,
        max_height: cfg.encode.max_height,
    }
}

/// Apply a preset to `cfg`. Devices, folders, shortcuts and the look are untouched.
pub fn apply(p: &Preset, cfg: &mut Config) {
    cfg.capture.target_fps = p.fps.clamp(1, 240);
    cfg.capture.show_cursor = p.cursor;
    cfg.audio.desktop_enabled = p.desktop_sound;
    cfg.audio.mic_enabled = p.mic;
    cfg.audio.tracks = if p.separate_tracks {
        AudioTracks::Separate
    } else {
        AudioTracks::Mixed
    };
    cfg.buffer.replay_duration_secs = p.replay_secs.clamp(1, 300);
    cfg.encode.bitrate_kbps = p.bitrate_kbps.clamp(1_000, 100_000);
    cfg.encode.gop_frames = (p.keyframe_secs.max(1) * cfg.capture.target_fps).max(1);
    cfg.encode.gpu_convert = p.gpu_convert;
    cfg.encode.max_height = p.max_height;
}

fn file() -> PathBuf {
    Config::default_path()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("presets.json")))
        .unwrap_or_else(|| PathBuf::from("presets.json"))
}

/// The user's own presets (empty if there are none or the file is unreadable).
pub fn load() -> Vec<Preset> {
    std::fs::read(file())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Remember the user's presets.
pub fn save(presets: &[Preset]) -> Result<(), String> {
    let path = file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(presets).map_err(|e| e.to_string())?;
    // Write beside, then rename, so a crash never leaves half a file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Add a preset, or replace the one with the same name.
pub fn upsert(list: &mut Vec<Preset>, preset: Preset) {
    match list
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&preset.name))
    {
        Some(existing) => *existing = preset,
        None => list.push(preset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_then_apply_round_trips() {
        let mut cfg = Config::default();
        cfg.capture.target_fps = 144;
        cfg.encode.bitrate_kbps = 15_000;
        cfg.encode.gop_frames = 288; // two seconds at 144
        let p = capture(&cfg, "  Mine ");
        assert_eq!(p.name, "Mine");
        assert_eq!(p.keyframe_secs, 2);
        let mut other = Config::default();
        apply(&p, &mut other);
        assert_eq!(other.capture.target_fps, 144);
        assert_eq!(other.encode.bitrate_kbps, 15_000);
        assert_eq!(other.encode.gop_frames, 288);
    }

    #[test]
    fn applying_never_touches_devices_or_look() {
        let mut cfg = Config::default();
        cfg.capture.monitor_id = "DISPLAY2".into();
        cfg.audio.desktop_device = "Headphones".into();
        cfg.general.theme = "ember".into();
        cfg.hotkeys.save = "CTRL+G".into();
        for p in built_in() {
            apply(&p, &mut cfg);
        }
        assert_eq!(cfg.capture.monitor_id, "DISPLAY2");
        assert_eq!(cfg.audio.desktop_device, "Headphones");
        assert_eq!(cfg.general.theme, "ember");
        assert_eq!(cfg.hotkeys.save, "CTRL+G");
    }

    #[test]
    fn built_in_presets_are_valid_configs() {
        for p in built_in() {
            let mut cfg = Config::default();
            apply(&p, &mut cfg);
            assert!(cfg.validate().is_ok(), "{} should be valid", p.name);
        }
    }

    #[test]
    fn upsert_replaces_by_name_ignoring_case() {
        let mut list = vec![capture(&Config::default(), "Fast")];
        let mut changed = capture(&Config::default(), "fast");
        changed.fps = 120;
        upsert(&mut list, changed);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].fps, 120);
    }
}
