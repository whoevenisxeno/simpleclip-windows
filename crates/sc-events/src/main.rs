#![cfg_attr(windows, windows_subsystem = "windows")]

mod cs2;
mod engine;
mod ipc;
mod league;
mod parent;

use engine::{describe_kills, Engine, Game};
use sc_core::addon::{self, EventsStatus};
use sc_core::config::{Config, EventsSettings};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const CONNECTED_FOR: u64 = 15;

#[derive(Default)]
struct Shared {
    cs2_port_busy: AtomicBool,
    cs2_last_payload: AtomicU64,
    league_wanted: AtomicBool,
    league_in_game: AtomicBool,
}

struct Options {
    parent: Option<u32>,
    dir: Option<PathBuf>,
    config: Option<PathBuf>,
    port: u16,
    no_cfg: bool,
}

fn parse_args(args: &[String]) -> Options {
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    Options {
        parent: value("--parent").and_then(|v| v.parse().ok()),
        dir: value("--dir").map(PathBuf::from),
        config: value("--config").map(PathBuf::from),
        port: value("--port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cs2::PORT),
        no_cfg: args.iter().any(|a| a == "--no-cfg"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--version") {
        println!("{VERSION}");
        return;
    }
    if has("--uninstall") {
        cs2::remove(&cs2::locate());
        return;
    }
    let options = parse_args(&args);
    let _log = init_logging();
    tracing::info!(version = VERSION, "sc-events starting");
    run(options);
    tracing::info!("sc-events stopping");
}

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let dir = directories_logs()?;
    let appender = tracing_appender::rolling::daily(dir, "sc-events.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();
    Some(guard)
}

fn directories_logs() -> Option<PathBuf> {
    addon::events_dir()?
        .parent()?
        .parent()
        .map(|p| p.join("logs"))
}

fn read_settings(path: &Option<PathBuf>, previous: &EventsSettings) -> EventsSettings {
    path.as_ref()
        .and_then(|p| Config::load(p).ok())
        .map(|c| c.events)
        .unwrap_or_else(|| previous.clone())
}

fn run(options: Options) {
    let Some(dir) = options.dir.clone().or_else(addon::events_dir) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let config_path = options
        .config
        .clone()
        .or_else(|| Config::default_path().ok());
    let port = options.port;
    let mut settings = read_settings(&config_path, &EventsSettings::default());
    let shared = Arc::new(Shared::default());
    let (tx, rx) = mpsc::channel::<(Game, u32)>();

    {
        let (tx, shared) = (tx.clone(), shared.clone());
        std::thread::spawn(move || match cs2::bind(port) {
            Ok(listener) => {
                let mut tracker = cs2::Tracker::default();
                cs2::serve(listener, |payload| {
                    shared
                        .cs2_last_payload
                        .store(addon::now_secs(), Ordering::Relaxed);
                    if let Some(kills) = tracker.feed(&payload) {
                        let _ = tx.send((Game::Cs2, kills));
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, port, "CS2 port is busy");
                shared.cs2_port_busy.store(true, Ordering::Relaxed);
            }
        });
    }
    {
        let (tx, shared) = (tx, shared.clone());
        std::thread::spawn(move || {
            let mut tracker = league::Tracker::default();
            loop {
                if !shared.league_wanted.load(Ordering::Relaxed) {
                    shared.league_in_game.store(false, Ordering::Relaxed);
                    tracker.reset();
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                }
                match league::poll() {
                    Some(snapshot) => {
                        shared.league_in_game.store(true, Ordering::Relaxed);
                        if let Some(kills) = tracker.feed(&snapshot.me, &snapshot.events) {
                            let _ = tx.send((Game::League, kills));
                        }
                        std::thread::sleep(Duration::from_secs(2));
                    }
                    None => {
                        shared.league_in_game.store(false, Ordering::Relaxed);
                        tracker.reset();
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        });
    }

    let mut engine = Engine::new(settings.clone(), ipc::DaemonSaver);
    let mut cs2_dirs: Vec<PathBuf> = Vec::new();
    let mut last_event: Option<String> = None;
    let long_ago = Instant::now() - Duration::from_secs(3600);
    let (mut last_reload, mut last_cfg, mut last_status) = (long_ago, long_ago, long_ago);
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok((game, kills)) => engine.kill(game, kills, Instant::now()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        if let Some(out) = engine.tick(now) {
            let what = format!("{} in {}", describe_kills(out.kills), out.game.name());
            last_event = Some(match &out.result {
                Ok(()) => format!("{what}, clip saved"),
                Err(e) => format!("{what}, could not save: {e}"),
            });
            tracing::info!(event = ?last_event, "event handled");
            last_status = long_ago;
        }
        if now.duration_since(last_reload) > Duration::from_secs(3) {
            last_reload = now;
            let fresh = read_settings(&config_path, &settings);
            if fresh != settings {
                settings = fresh.clone();
                engine.set_settings(fresh);
                last_cfg = long_ago;
            }
            shared
                .league_wanted
                .store(settings.enabled && settings.league, Ordering::Relaxed);
        }
        if settings.enabled
            && settings.cs2
            && !options.no_cfg
            && now.duration_since(last_cfg) > Duration::from_secs(60)
        {
            last_cfg = now;
            cs2_dirs = cs2::locate();
            cs2::install(&cs2_dirs, port);
        }
        if let Some(pid) = options.parent {
            if !parent::alive(pid) {
                break;
            }
        }
        if now.duration_since(last_status) > Duration::from_secs(2) {
            last_status = now;
            let status = status_now(&settings, &shared, &cs2_dirs, &last_event);
            let _ = addon::write_status(&dir, &status);
        }
    }
}

fn status_now(
    settings: &EventsSettings,
    shared: &Shared,
    cs2_dirs: &[PathBuf],
    last_event: &Option<String>,
) -> EventsStatus {
    let now = addon::now_secs();
    let cs2 = if !(settings.enabled && settings.cs2) {
        "off"
    } else if shared.cs2_port_busy.load(Ordering::Relaxed) {
        "port-busy"
    } else if cs2_dirs.is_empty() {
        "not-found"
    } else if now.saturating_sub(shared.cs2_last_payload.load(Ordering::Relaxed)) <= CONNECTED_FOR {
        "connected"
    } else {
        "waiting"
    };
    let league = if !(settings.enabled && settings.league) {
        "off"
    } else if shared.league_in_game.load(Ordering::Relaxed) {
        "in-game"
    } else {
        "waiting"
    };
    EventsStatus {
        version: VERSION.to_string(),
        cs2: cs2.to_string(),
        league: league.to_string(),
        last_event: last_event.clone(),
        updated: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> EventsSettings {
        EventsSettings {
            enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_status_says_off_when_the_add_on_is_off() {
        let shared = Shared::default();
        let s = status_now(&EventsSettings::default(), &shared, &[], &None);
        assert_eq!((s.cs2.as_str(), s.league.as_str()), ("off", "off"));
    }

    #[test]
    fn the_status_follows_what_the_games_are_doing() {
        let shared = Shared::default();
        let dirs = vec![PathBuf::from("x")];
        let s = status_now(&on(), &shared, &dirs, &None);
        assert_eq!((s.cs2.as_str(), s.league.as_str()), ("waiting", "waiting"));
        shared
            .cs2_last_payload
            .store(addon::now_secs(), Ordering::Relaxed);
        shared.league_in_game.store(true, Ordering::Relaxed);
        let s = status_now(&on(), &shared, &dirs, &Some("Double kill".into()));
        assert_eq!(
            (s.cs2.as_str(), s.league.as_str()),
            ("connected", "in-game")
        );
        assert_eq!(s.last_event.as_deref(), Some("Double kill"));
        shared
            .cs2_last_payload
            .store(addon::now_secs() - 60, Ordering::Relaxed);
        let s = status_now(&on(), &shared, &dirs, &None);
        assert_eq!(s.cs2, "waiting");
    }

    #[test]
    fn the_status_reports_setup_problems() {
        let shared = Shared::default();
        let s = status_now(&on(), &shared, &[], &None);
        assert_eq!(s.cs2, "not-found");
        shared.cs2_port_busy.store(true, Ordering::Relaxed);
        let s = status_now(&on(), &shared, &[], &None);
        assert_eq!(s.cs2, "port-busy");
    }

    #[test]
    fn a_game_can_be_unticked_on_its_own() {
        let shared = Shared::default();
        let mut s = on();
        s.league = false;
        let status = status_now(&s, &shared, &[PathBuf::from("x")], &None);
        assert_eq!(
            (status.cs2.as_str(), status.league.as_str()),
            ("waiting", "off")
        );
    }

    #[test]
    fn settings_fall_back_when_the_file_cannot_be_read() {
        let previous = EventsSettings {
            min_kills: 4,
            ..Default::default()
        };
        assert_eq!(read_settings(&None, &previous), previous);
        let missing = Some(PathBuf::from("definitely-not-here.toml"));
        assert_eq!(read_settings(&missing, &previous), previous);
    }
}

#[cfg(test)]
mod end_to_end {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    struct Count(Arc<std::sync::Mutex<Vec<u32>>>);

    impl engine::Saver for Count {
        fn save(&mut self, secs: u32) -> Result<(), String> {
            self.0.lock().unwrap().push(secs);
            Ok(())
        }
    }

    fn send(port: u16, round: i64, kills: u32) {
        let me = "76561198000000001";
        let body = format!(
            r#"{{"provider":{{"steamid":"{me}"}},"map":{{"round":{round}}},"player":{{"steamid":"{me}","state":{{"round_kills":{kills}}}}}}}"#
        );
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
    }

    #[test]
    fn a_triple_kill_in_cs2_ends_up_as_one_saved_clip() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel::<(Game, u32)>();
        std::thread::spawn(move || {
            let mut tracker = cs2::Tracker::default();
            cs2::serve(listener, |p| {
                if let Some(k) = tracker.feed(&p) {
                    let _ = tx.send((Game::Cs2, k));
                }
            });
        });
        for (round, kills) in [(7, 0), (7, 0), (7, 1), (7, 2), (7, 3), (8, 0)] {
            send(port, round, kills);
        }
        let saves = Arc::new(std::sync::Mutex::new(Vec::new()));
        let settings = EventsSettings {
            enabled: true,
            min_kills: 3,
            save_secs: 25,
            ..Default::default()
        };
        let mut engine = Engine::new(settings, Count(saves.clone()));
        let t0 = Instant::now();
        let mut seen = Vec::new();
        while let Ok((game, kills)) = rx.recv_timeout(Duration::from_millis(300)) {
            seen.push(kills);
            engine.kill(game, kills, t0);
        }
        assert_eq!(seen, vec![1, 2, 3]);
        let out = engine.tick(t0 + Duration::from_secs(5)).unwrap();
        assert_eq!(out.kills, 3);
        assert_eq!(*saves.lock().unwrap(), vec![25]);
        assert!(engine.tick(t0 + Duration::from_secs(60)).is_none());
    }
}
