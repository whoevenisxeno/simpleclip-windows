use sc_core::config::Config;
#[cfg(windows)]
use sc_core::encode::EncoderKind;
use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;

#[cfg(windows)]
pub struct SavedClip<'a> {
    pub path: &'a Path,
    pub secs: f64,
    pub app: Option<&'a str>,
    pub encoder: EncoderKind,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
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
        crate::popup::configure(&cfg.general);
        let minimal = cfg.general.popup_style == sc_core::config::PopupStyle::Minimal;
        crate::popup::show(crate::popup::Popup {
            kind: crate::popup::Kind::Saved,
            compact: minimal,
            title: "Clip saved".into(),
            lines: if minimal {
                vec![format!("{:.0}s", c.secs)]
            } else {
                vec![
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
                ]
            },
        });
    }
    if cfg.general.save_sound {
        crate::win::play_save_sound(&cfg.general);
    }
}

#[cfg(windows)]
pub fn screenshot_saved(path: &Path, width: u32, height: u32, cfg: &Config) {
    if cfg.general.notify {
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        crate::popup::configure(&cfg.general);
        let minimal = cfg.general.popup_style == sc_core::config::PopupStyle::Minimal;
        crate::popup::show(crate::popup::Popup {
            kind: crate::popup::Kind::Saved,
            compact: minimal,
            title: "Screenshot saved".into(),
            lines: if minimal {
                Vec::new()
            } else {
                vec![
                    file_name(path).to_string(),
                    format!(
                        "{width}x{height}  \u{2022}  {:.1} MB",
                        bytes as f64 / (1024.0 * 1024.0)
                    ),
                ]
            },
        });
    }
    if cfg.general.save_sound {
        crate::win::play_save_sound(&cfg.general);
    }
}

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

#[cfg(windows)]
pub fn test(cfg: &Config) {
    crate::popup::configure(&cfg.general);
    let minimal = cfg.general.popup_style == sc_core::config::PopupStyle::Minimal;
    crate::popup::show(crate::popup::Popup {
        kind: crate::popup::Kind::Saved,
        compact: minimal,
        title: "Clip saved".into(),
        lines: if minimal {
            vec!["12s".into()]
        } else {
            vec![
                "sc-sample-20260101-120000.mp4".into(),
                "Sample game  \u{2022}  12s  \u{2022}  13.9 MB".into(),
                "1920x1080 @ 60 fps  \u{2022}  AMD AMF  \u{2022}  9.8 Mbps".into(),
                "Saved in 0.1s  \u{2022}  \\sample".into(),
            ]
        },
    });
    if cfg.general.save_sound {
        crate::win::play_save_sound(&cfg.general);
    }
}
