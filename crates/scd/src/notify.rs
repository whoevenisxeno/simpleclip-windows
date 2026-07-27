use sc_core::config::Config;
use std::path::Path;
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

    if cfg.general.save_sound {
        run_detached(save_sound_command());
    }
}

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

fn run_detached(mut cmd: Command) {
    std::thread::spawn(move || {
        let _ = cmd.status();
    });
}
