use sc_core::config::Config;
#[cfg(windows)]
use sc_core::encode::EncoderKind;
use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;

/// Everything worth telling the user about a freshly saved clip.
#[cfg(windows)]
pub struct SavedClip<'a> {
    pub path: &'a Path,
    /// Length of the clip in seconds.
    pub secs: f64,
    pub app: Option<&'a str>,
    pub encoder: EncoderKind,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// How long snapshotting and muxing took.
    pub took: std::time::Duration,
}

#[cfg(windows)]
fn encoder_label(kind: EncoderKind) -> &'static str {
    match kind {
        EncoderKind::Nvenc => "NVENC",
        EncoderKind::Amf => "AMD AMF",
        EncoderKind::Qsv => "Intel QSV",
        EncoderKind::Vaapi => "VA-API",
        EncoderKind::Software => "software",
    }
}

#[cfg(windows)]
fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("clip")
}

/// Pop down the "clip saved" card (with stats) and play the save sound.
#[cfg(windows)]
pub fn clip_saved(c: &SavedClip, cfg: &Config) {
    if cfg.general.notify {
        let bytes = std::fs::metadata(c.path).map(|m| m.len()).unwrap_or(0);
        let mb = bytes as f64 / (1024.0 * 1024.0);
        let mbps = if c.secs > 0.0 {
            bytes as f64 * 8.0 / c.secs / 1_000_000.0
        } else {
            0.0
        };
        let game = c.app.unwrap_or("Desktop");
        let place = c
            .path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("");
        crate::popup::show(crate::popup::Popup {
            ok: true,
            title: "Clip saved".into(),
            lines: vec![
                file_name(c.path).to_string(),
                format!("{game}  \u{2022}  {:.0}s  \u{2022}  {mb:.1} MB", c.secs),
                format!(
                    "{}x{} @ {} fps  \u{2022}  {}  \u{2022}  {mbps:.1} Mbps",
                    c.width,
                    c.height,
                    c.fps,
                    encoder_label(c.encoder)
                ),
                format!("Saved in {:.1}s  \u{2022}  \\{place}", c.took.as_secs_f64()),
            ],
        });
    }
    if cfg.general.save_sound {
        crate::win::play_save_sound();
    }
}

/// Notification for a saved screenshot.
#[cfg(windows)]
pub fn screenshot_saved(path: &Path, width: u32, height: u32, cfg: &Config) {
    if cfg.general.notify {
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        crate::popup::show(crate::popup::Popup {
            ok: true,
            title: "Screenshot saved".into(),
            lines: vec![
                file_name(path).to_string(),
                format!(
                    "{width}x{height}  \u{2022}  {:.1} MB",
                    bytes as f64 / (1024.0 * 1024.0)
                ),
            ],
        });
    }
    if cfg.general.save_sound {
        crate::win::play_save_sound();
    }
}

/// Fire a desktop notification and (optionally) a sound when a clip is saved.
/// Everything runs detached on a short-lived thread so the daemon never blocks
/// and child processes are reaped instead of leaking as zombies.
#[cfg(not(windows))]
pub fn clip_saved(path: &Path, secs: f64, cfg: &Config) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clip")
        .to_string();

    if cfg.general.notify {
        let mut n = Command::new("notify-send");
        n.args([
            "-a",
            "SimpleClip",
            "-i",
            "camera-video",
            "-t",
            "3000",
            "Clip saved",
        ])
        .arg(format!("{name}  ({secs:.0}s)"));
        run_detached(n);
    }

    if cfg.general.save_sound {
        run_detached(save_sound_command());
    }
}

#[cfg(not(windows))]
fn save_sound_command() -> Command {
    const FREEDESKTOP: &str = "/usr/share/sounds/freedesktop/stereo/screen-capture.oga";
    if Path::new(FREEDESKTOP).exists() {
        let mut c = Command::new("paplay");
        c.arg(FREEDESKTOP);
        c
    } else {
        let mut c = Command::new("canberra-gtk-play");
        c.args(["-i", "screen-capture"]);
        c
    }
}

#[cfg(not(windows))]
fn run_detached(mut cmd: Command) {
    std::thread::spawn(move || {
        let _ = cmd.status();
    });
}
