use sc_core::config::Config;
use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;

/// Fire a desktop notification and (optionally) a sound when a clip is saved.
/// Everything runs detached on a short-lived thread so the daemon never blocks
/// and child processes are reaped instead of leaking as zombies.
pub fn clip_saved(path: &Path, secs: f64, cfg: &Config) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clip")
        .to_string();

    #[cfg(windows)]
    {
        if cfg.general.notify {
            crate::win::notify("Clip saved", &format!("{name}  ({secs:.0}s)"));
        }
        if cfg.general.save_sound {
            crate::win::play_save_sound();
        }
    }

    #[cfg(not(windows))]
    {
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
}

/// Notification for a saved screenshot.
#[cfg(windows)]
pub fn screenshot_saved(path: &Path, cfg: &Config) {
    if cfg.general.notify {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("screenshot");
        crate::win::notify("Screenshot saved", name);
    }
    if cfg.general.save_sound {
        crate::win::play_save_sound();
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
