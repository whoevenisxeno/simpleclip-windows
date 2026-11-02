#![cfg_attr(windows, windows_subsystem = "windows")]

mod parent;
#[cfg(windows)]
mod watch;
#[cfg(windows)]
mod win;

use sc_core::store;
use sc_keys::settings::{self, Settings};
use sc_keys::{burn, draw, preview, tape};
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Options {
    parent: Option<u32>,
    dir: Option<PathBuf>,
    clip: Option<PathBuf>,
    start: f64,
    events: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Options {
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    Options {
        parent: value("--parent").and_then(|v| v.parse().ok()),
        dir: value("--dir").map(PathBuf::from),
        clip: value("--burn").map(PathBuf::from),
        start: value("--start").and_then(|v| v.parse().ok()).unwrap_or(0.0),
        events: value("--events").map(PathBuf::from),
    }
}

fn burn_one(dir: &Path, clip: &Path, o: &Options) -> Result<burn::Outcome, String> {
    let s = Settings::read(&settings::manifest(), &store::read_settings(dir));
    let text = o
        .events
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .ok_or("the key list is missing")?;
    let events = tape::decode(&text);
    burn::burn(
        clip,
        o.start,
        &events,
        &s,
        draw::font().as_ref(),
        &dir.join("work"),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--version") {
        attach_console();
        println!("{VERSION}");
        return;
    }
    if has("--uninstall") {
        return;
    }
    let options = parse_args(&args);
    let Some(dir) = options
        .dir
        .clone()
        .or_else(|| store::root().map(|r| r.join("keys")))
    else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    if has("--preview-only") {
        let s = Settings::read(&settings::manifest(), &store::read_settings(&dir));
        let ok = preview::write(&dir, &s.shape(), draw::font().as_ref()).is_ok();
        std::process::exit(if ok { 0 } else { 1 });
    }
    if let Some(clip) = options.clip.as_ref() {
        attach_console();
        let done = burn_one(&dir, clip, &options);
        println!("{}", burn::report(&done));
        return;
    }
    let _log = init_logging(&dir);
    tracing::info!(version = VERSION, "sc-keys starting");
    run(&dir, options.parent);
    tracing::info!("sc-keys stopping");
}

#[cfg(windows)]
fn run(dir: &Path, parent: Option<u32>) {
    win::run(dir, parent);
}

#[cfg(not(windows))]
fn run(dir: &Path, _parent: Option<u32>) {
    let _ = store::write_status(dir, "Off");
}

#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

fn init_logging(dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let logs = dir.parent()?.parent()?.join("logs");
    let appender = tracing_appender::rolling::daily(logs, "sc-keys.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();
    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arguments_are_understood() {
        let args: Vec<String> = ["--parent", "42", "--dir", "d"].map(String::from).to_vec();
        let o = parse_args(&args);
        assert_eq!(o.parent, Some(42));
        assert_eq!(o.dir, Some(PathBuf::from("d")));
        let none = parse_args(&[]);
        assert!(none.parent.is_none() && none.dir.is_none() && none.clip.is_none());
        let args: Vec<String> = ["--burn", "c.mp4", "--start", "12.5", "--events", "e.txt"]
            .map(String::from)
            .to_vec();
        let o = parse_args(&args);
        assert_eq!(o.clip, Some(PathBuf::from("c.mp4")));
        assert_eq!(o.start, 12.5);
        assert_eq!(o.events, Some(PathBuf::from("e.txt")));
    }
}
