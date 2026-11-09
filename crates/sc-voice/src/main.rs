#![cfg_attr(windows, windows_subsystem = "windows")]

mod hear;
mod ipc;
mod parent;
#[cfg(windows)]
mod sapi;
#[cfg(not(windows))]
#[path = "nosapi.rs"]
mod sapi;

use hear::{Cooldown, Ear, Heard, Last, Phrase, Settings, Strictness, COOLDOWN};
use sapi::{Got, Listener, Problem, Source};
use sc_core::store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Options {
    parent: Option<u32>,
    dir: Option<PathBuf>,
    listen: Option<PathBuf>,
    feed: Option<PathBuf>,
    phrase: Phrase,
}

fn parse_args(args: &[String]) -> Options {
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    Options {
        parent: value("--parent").and_then(|v| v.parse().ok()),
        dir: value("--dir").map(PathBuf::from),
        listen: value("--listen-file").map(PathBuf::from),
        feed: value("--feed-file").map(PathBuf::from),
        phrase: value("--phrase")
            .map(|p| Phrase::new(&p))
            .unwrap_or_default(),
    }
}

enum Note {
    Ready,
    Problem(Problem),
    Heard(Heard),
    Done,
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
    if let Some(wav) = options.listen.clone() {
        attach_console();
        std::process::exit(listen_file(&wav, &options.phrase));
    }
    let Some(dir) = options
        .dir
        .clone()
        .or_else(|| store::root().map(|r| r.join("voice")))
    else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let _log = init_logging(&dir);
    tracing::info!(version = VERSION, "sc-voice starting");
    run(&dir, &options);
    tracing::info!("sc-voice stopping");
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
    let appender = tracing_appender::rolling::daily(logs, "sc-voice.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();
    Some(guard)
}

fn listen_file(wav: &Path, phrase: &Phrase) -> i32 {
    sapi::init();
    let l = match Listener::open(Source::File(wav), phrase) {
        Ok(l) => l,
        Err(p) => {
            eprintln!("{}", describe(p));
            return 2;
        }
    };
    loop {
        let Ok(got) = l.wait(10_000) else {
            eprintln!("the recognizer stopped");
            return 2;
        };
        if got.is_empty() {
            eprintln!("the recognizer timed out");
            return 2;
        }
        for g in got {
            match g {
                Got::Heard(h) => {
                    let yes = if hear::accepts(&h, phrase, Strictness::Normal) {
                        "yes"
                    } else {
                        "no"
                    };
                    let kind = if h.command { "command" } else { "other" };
                    let words: Vec<String> = h.words.iter().map(|w| format!("{w:.2}")).collect();
                    println!(
                        "{yes}\t{:.3}\t{kind}\t{}\t{}",
                        h.confidence,
                        h.text,
                        words.join(" ")
                    );
                }
                Got::End => return 0,
            }
        }
    }
}

fn describe(p: Problem) -> &'static str {
    match p {
        Problem::NoSpeech => "Windows speech recognition is not available",
        Problem::NoMic => "No microphone found",
        Problem::BadFile => "the sound file could not be read",
    }
}

type Shared = Arc<Mutex<Phrase>>;

fn current(p: &Shared) -> Phrase {
    p.lock().map(|p| p.clone()).unwrap_or_default()
}

fn spawn_listener(
    file: Option<PathBuf>,
    wanted: Arc<AtomicBool>,
    phrase: Shared,
    tx: mpsc::Sender<Note>,
) {
    std::thread::spawn(move || {
        sapi::init();
        listen(file.as_deref(), &wanted, &phrase, &tx);
        let _ = tx.send(Note::Done);
    });
}

fn listen(file: Option<&Path>, wanted: &AtomicBool, phrase: &Shared, tx: &mpsc::Sender<Note>) {
    let mic = file.is_none();
    loop {
        if mic && !wanted.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let src = file.map(Source::File).unwrap_or(Source::Mic);
        let using = current(phrase);
        let l = match Listener::open(src, &using) {
            Ok(l) => l,
            Err(p) => {
                tracing::warn!(problem = describe(p), "could not start listening");
                if tx.send(Note::Problem(p)).is_err() || !mic {
                    return;
                }
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
        };
        if tx.send(Note::Ready).is_err() {
            return;
        }
        let mut checked = Instant::now();
        'live: loop {
            if mic && (!wanted.load(Ordering::Relaxed) || current(phrase) != using) {
                break;
            }
            if mic && checked.elapsed() > Duration::from_secs(3) {
                checked = Instant::now();
                if !sapi::has_mic() {
                    break;
                }
            }
            let Ok(got) = l.wait(500) else { break };
            for g in got {
                match g {
                    Got::Heard(h) => {
                        if tx.send(Note::Heard(h)).is_err() {
                            return;
                        }
                    }
                    Got::End if mic => break 'live,
                    Got::End => return,
                }
            }
        }
        drop(l);
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn beep() {
    #[cfg(windows)]
    std::thread::spawn(|| unsafe {
        use windows::Win32::System::Diagnostics::Debug::Beep;
        let _ = Beep(1320, 70);
        let _ = Beep(1760, 90);
    });
}

fn tag_saved(dir: &Path, clip: Option<&Path>) {
    let (Some(clip), Some(root)) = (clip, dir.parent()) else {
        return;
    };
    if let Err(e) = store::tag_clip(root, clip, hear::TAG) {
        tracing::warn!(error = %e, "could not tag the clip");
    }
}

fn run(dir: &Path, options: &Options) {
    let manifest = hear::manifest();
    let mut settings = Settings::read(&manifest, &store::read_settings(dir));
    let wanted = Arc::new(AtomicBool::new(settings.enabled));
    let (tx, rx) = mpsc::channel::<Note>();
    let phrase: Shared = Arc::new(Mutex::new(settings.phrase.clone()));
    spawn_listener(options.feed.clone(), wanted.clone(), phrase.clone(), tx);

    let mut mic = Ear::Starting;
    let mut last: Option<Last> = None;
    let mut cooldown = Cooldown::new(COOLDOWN);
    let mut pending: Option<(Instant, Option<u32>)> = None;
    let long_ago = Instant::now() - Duration::from_secs(3600);
    let (mut last_reload, mut last_status) = (Instant::now(), long_ago);
    loop {
        let mut done = false;
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Note::Ready) => {
                mic = Ear::Listening;
                last_status = long_ago;
            }
            Ok(Note::Problem(p)) => {
                mic = match p {
                    Problem::NoMic => Ear::NoMic,
                    _ => Ear::NoSpeech,
                };
                last_status = long_ago;
            }
            Ok(Note::Heard(h)) => {
                let now = Instant::now();
                let ok =
                    settings.enabled && hear::accepts(&h, &settings.phrase, settings.strictness);
                tracing::info!(text = %h.text, confidence = h.confidence, words = ?h.words, command = h.command, ok, "heard");
                if ok && cooldown.take(now) {
                    if settings.beep {
                        beep();
                    }
                    if settings.card {
                        if let Err(e) = ipc::popup(
                            "Saving clip",
                            &format!("Heard \u{201C}{}\u{201D}", settings.phrase.text()),
                        ) {
                            tracing::warn!(error = %e, "could not show the card");
                        }
                    }
                    pending = Some((now + hear::save_delay(&settings), settings.secs));
                }
            }
            Ok(Note::Done) | Err(mpsc::RecvTimeoutError::Disconnected) => done = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if let Some((at, secs)) = pending.filter(|(at, _)| now >= *at) {
            pending = None;
            let result = match ipc::save(secs) {
                Ok(clip) => {
                    tracing::info!(secs = ?secs, "clip saved");
                    tag_saved(dir, clip.as_deref());
                    Ok(())
                }
                Err(e) => {
                    tracing::warn!(error = %e, "could not save");
                    Err(e)
                }
            };
            last = Some(Last { at, result });
            last_status = long_ago;
        }
        if now.duration_since(last_reload) > Duration::from_secs(3) {
            last_reload = now;
            settings = Settings::read(&manifest, &store::read_settings(dir));
            wanted.store(settings.enabled, Ordering::Relaxed);
            if let Ok(mut p) = phrase.lock() {
                if *p != settings.phrase {
                    tracing::info!(phrase = %settings.phrase.text(), "new phrase");
                    *p = settings.phrase.clone();
                }
            }
        }
        if let Some(pid) = options.parent {
            if !parent::alive(pid) {
                break;
            }
        }
        if done || now.duration_since(last_status) > Duration::from_secs(2) {
            last_status = now;
            let line =
                hear::status_line(settings.enabled, &settings.phrase, mic, last.as_ref(), now);
            let _ = store::write_status(dir, &line);
        }
        if done {
            if pending.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arguments_are_understood() {
        let args: Vec<String> = ["--parent", "42", "--dir", "d", "--feed-file", "f.wav"]
            .map(String::from)
            .to_vec();
        let o = parse_args(&args);
        assert_eq!(o.parent, Some(42));
        assert_eq!(o.dir, Some(PathBuf::from("d")));
        assert_eq!(o.feed, Some(PathBuf::from("f.wav")));
        assert_eq!(o.listen, None);
        let none = parse_args(&[]);
        assert!(none.parent.is_none() && none.dir.is_none() && none.feed.is_none());
    }

    #[test]
    fn a_saved_clip_is_tagged_where_simpleclip_looks() {
        let root = std::env::temp_dir().join(format!("sc-voice-tag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("voice");
        std::fs::create_dir_all(&dir).unwrap();
        tag_saved(&dir, None);
        assert!(store::take_clip_tags(&root).is_empty());
        tag_saved(&dir, Some(Path::new("clip.mp4")));
        let got = store::take_clip_tags(&root);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, PathBuf::from("clip.mp4"));
        assert_eq!(got[0].tag, "Clip that");
        let _ = std::fs::remove_dir_all(&root);
    }
}
