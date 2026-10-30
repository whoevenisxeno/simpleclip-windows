#![cfg_attr(windows, windows_subsystem = "windows")]

mod cs2;
mod dota;
mod engine;
mod ipc;
mod league;
#[cfg(windows)]
mod ocr;
mod parent;
mod proc;
mod r6;

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
    port_busy: AtomicBool,
    cs2_last_payload: AtomicU64,
    dota_last_payload: AtomicU64,
    league_wanted: AtomicBool,
    league_in_game: AtomicBool,
    r6_wanted: AtomicBool,
    r6_running: AtomicBool,
    r6_reading: AtomicBool,
    ocr_missing: AtomicBool,
}

#[derive(Default)]
struct Setup {
    cs2_dirs: Vec<PathBuf>,
    dota_dirs: Vec<PathBuf>,
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
        dota::remove(&dota::locate());
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

fn spawn_state_feed(port: u16, tx: mpsc::Sender<(Game, u32)>, shared: Arc<Shared>) {
    std::thread::spawn(move || match cs2::bind(port) {
        Ok(listener) => {
            let mut cs2_tracker = cs2::Tracker::default();
            let mut dota_tracker = dota::Tracker::default();
            cs2::serve(listener, |body| {
                if let Ok(p) = serde_json::from_slice::<cs2::Payload>(&body) {
                    if p.is_cs2() {
                        shared
                            .cs2_last_payload
                            .store(addon::now_secs(), Ordering::Relaxed);
                    }
                    if let Some(kills) = cs2_tracker.feed(&p) {
                        let _ = tx.send((Game::Cs2, kills));
                    }
                }
                if let Ok(p) = serde_json::from_slice::<dota::Payload>(&body) {
                    if p.is_dota() {
                        shared
                            .dota_last_payload
                            .store(addon::now_secs(), Ordering::Relaxed);
                    }
                    if let Some(kills) = dota_tracker.feed(&p, Instant::now()) {
                        let _ = tx.send((Game::Dota2, kills));
                    }
                }
            });
        }
        Err(e) => {
            tracing::warn!(error = %e, port, "the game state port is busy");
            shared.port_busy.store(true, Ordering::Relaxed);
        }
    });
}

fn spawn_league(tx: mpsc::Sender<(Game, u32)>, shared: Arc<Shared>) {
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

#[cfg(windows)]
fn spawn_r6(tx: mpsc::Sender<(Game, u32)>, shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let reader = ocr::Reader::new();
        if reader.is_none() {
            shared.ocr_missing.store(true, Ordering::Relaxed);
        }
        let mut tracker = r6::Tracker::default();
        let mut checked = Instant::now() - Duration::from_secs(60);
        loop {
            let Some(reader) = reader.as_ref() else {
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            if !shared.r6_wanted.load(Ordering::Relaxed) {
                shared.r6_running.store(false, Ordering::Relaxed);
                shared.r6_reading.store(false, Ordering::Relaxed);
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }
            if checked.elapsed() > Duration::from_secs(3) {
                checked = Instant::now();
                shared
                    .r6_running
                    .store(proc::running(r6::PROCESS), Ordering::Relaxed);
            }
            if !shared.r6_running.load(Ordering::Relaxed) {
                shared.r6_reading.store(false, Ordering::Relaxed);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            match ipc::grab_region(r6::REGION) {
                Ok((w, h, gray)) => {
                    shared.r6_reading.store(true, Ordering::Relaxed);
                    if let Some(lines) = reader.read(&gray, w, h) {
                        let reading = r6::read_feed(&lines, w as f32);
                        if let Some(kills) = tracker.feed(reading.kills, Instant::now()) {
                            let _ = tx.send((Game::R6, kills));
                        }
                    }
                    std::thread::sleep(Duration::from_millis(350));
                }
                Err(_) => {
                    shared.r6_reading.store(false, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn spawn_r6(_tx: mpsc::Sender<(Game, u32)>, _shared: Arc<Shared>) {}

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

    spawn_state_feed(port, tx.clone(), shared.clone());
    spawn_league(tx.clone(), shared.clone());
    spawn_r6(tx, shared.clone());

    let mut engine = Engine::new(settings.clone(), ipc::DaemonSaver);
    let mut setup = Setup::default();
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
            last_event = Some(match (&out.result, out.test) {
                (Ok(()), false) => format!("{what}, clip saved"),
                (Ok(()), true) => format!("{what}, test notification shown"),
                (Err(e), false) => format!("{what}, could not save: {e}"),
                (Err(e), true) => format!("{what}, could not show the test notification: {e}"),
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
            shared
                .r6_wanted
                .store(settings.enabled && settings.r6, Ordering::Relaxed);
        }
        if settings.enabled
            && !options.no_cfg
            && now.duration_since(last_cfg) > Duration::from_secs(60)
        {
            last_cfg = now;
            if settings.cs2 {
                setup.cs2_dirs = cs2::locate();
                cs2::install(&setup.cs2_dirs, port);
            }
            if settings.dota2 {
                setup.dota_dirs = dota::locate();
                dota::install(&setup.dota_dirs, port);
            }
        }
        if let Some(pid) = options.parent {
            if !parent::alive(pid) {
                break;
            }
        }
        if now.duration_since(last_status) > Duration::from_secs(2) {
            last_status = now;
            let status = status_now(&settings, &shared, &setup, &last_event);
            let _ = addon::write_status(&dir, &status);
        }
    }
}

fn gsi_status(
    wanted: bool,
    shared: &Shared,
    dirs: &[PathBuf],
    last_payload: &AtomicU64,
    now: u64,
) -> &'static str {
    if !wanted {
        "off"
    } else if shared.port_busy.load(Ordering::Relaxed) {
        "port-busy"
    } else if dirs.is_empty() {
        "not-found"
    } else if now.saturating_sub(last_payload.load(Ordering::Relaxed)) <= CONNECTED_FOR {
        "connected"
    } else {
        "waiting"
    }
}

fn status_now(
    settings: &EventsSettings,
    shared: &Shared,
    setup: &Setup,
    last_event: &Option<String>,
) -> EventsStatus {
    let now = addon::now_secs();
    let on = settings.enabled;
    let cs2 = gsi_status(
        on && settings.cs2,
        shared,
        &setup.cs2_dirs,
        &shared.cs2_last_payload,
        now,
    );
    let dota2 = gsi_status(
        on && settings.dota2,
        shared,
        &setup.dota_dirs,
        &shared.dota_last_payload,
        now,
    );
    let league = if !(on && settings.league) {
        "off"
    } else if shared.league_in_game.load(Ordering::Relaxed) {
        "in-game"
    } else {
        "waiting"
    };
    let r6 = if !(on && settings.r6) {
        "off"
    } else if shared.ocr_missing.load(Ordering::Relaxed) {
        "ocr-missing"
    } else if shared.r6_reading.load(Ordering::Relaxed) {
        "watching"
    } else {
        "waiting"
    };
    EventsStatus {
        version: VERSION.to_string(),
        cs2: cs2.to_string(),
        league: league.to_string(),
        r6: r6.to_string(),
        dota2: dota2.to_string(),
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

    fn found() -> Setup {
        Setup {
            cs2_dirs: vec![PathBuf::from("x")],
            dota_dirs: vec![PathBuf::from("y")],
        }
    }

    fn seen(s: &EventsStatus) -> [&str; 4] {
        [&s.cs2, &s.league, &s.r6, &s.dota2].map(String::as_str)
    }

    #[test]
    fn the_status_says_off_when_the_add_on_is_off() {
        let s = status_now(
            &EventsSettings::default(),
            &Shared::default(),
            &Setup::default(),
            &None,
        );
        assert_eq!(seen(&s), ["off"; 4]);
    }

    #[test]
    fn the_status_follows_what_the_games_are_doing() {
        let shared = Shared::default();
        let s = status_now(&on(), &shared, &found(), &None);
        assert_eq!(seen(&s), ["waiting"; 4]);
        let now = addon::now_secs();
        shared.cs2_last_payload.store(now, Ordering::Relaxed);
        shared.dota_last_payload.store(now, Ordering::Relaxed);
        shared.league_in_game.store(true, Ordering::Relaxed);
        shared.r6_reading.store(true, Ordering::Relaxed);
        let s = status_now(&on(), &shared, &found(), &Some("Double kill".into()));
        assert_eq!(seen(&s), ["connected", "in-game", "watching", "connected"]);
        assert_eq!(s.last_event.as_deref(), Some("Double kill"));
        shared.cs2_last_payload.store(now - 60, Ordering::Relaxed);
        let s = status_now(&on(), &shared, &found(), &None);
        assert_eq!(s.cs2, "waiting");
        assert_eq!(s.dota2, "connected");
    }

    #[test]
    fn the_status_reports_setup_problems() {
        let shared = Shared::default();
        let s = status_now(&on(), &shared, &Setup::default(), &None);
        assert_eq!(
            (s.cs2.as_str(), s.dota2.as_str()),
            ("not-found", "not-found")
        );
        shared.port_busy.store(true, Ordering::Relaxed);
        shared.ocr_missing.store(true, Ordering::Relaxed);
        let s = status_now(&on(), &shared, &Setup::default(), &None);
        assert_eq!(
            (s.cs2.as_str(), s.dota2.as_str()),
            ("port-busy", "port-busy")
        );
        assert_eq!(s.r6, "ocr-missing");
    }

    #[test]
    fn a_game_can_be_unticked_on_its_own() {
        let shared = Shared::default();
        let mut s = on();
        s.league = false;
        s.r6 = false;
        let status = status_now(&s, &shared, &found(), &None);
        assert_eq!(seen(&status), ["waiting", "off", "off", "waiting"]);
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

    #[test]
    fn the_arguments_are_understood() {
        let args: Vec<String> = [
            "--parent", "42", "--dir", "d", "--config", "c.toml", "--port", "9", "--no-cfg",
        ]
        .map(String::from)
        .to_vec();
        let o = parse_args(&args);
        assert_eq!(o.parent, Some(42));
        assert_eq!(o.dir, Some(PathBuf::from("d")));
        assert_eq!(o.config, Some(PathBuf::from("c.toml")));
        assert_eq!((o.port, o.no_cfg), (9, true));
        let none = parse_args(&[]);
        assert_eq!(
            (none.parent, none.port, none.no_cfg),
            (None, cs2::PORT, false)
        );
    }
}

#[cfg(test)]
mod end_to_end {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    struct Count(Arc<std::sync::Mutex<Vec<u32>>>);

    impl engine::Saver for Count {
        fn save(&mut self, secs: u32) -> Result<(), String> {
            self.0.lock().unwrap().push(secs);
            Ok(())
        }

        fn announce(&mut self, _title: &str, _line: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn send(port: u16, body: &str) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
    }

    fn cs2_body(round: i64, kills: u32) -> String {
        let me = "76561198000000001";
        format!(
            r#"{{"provider":{{"steamid":"{me}","appid":730}},"map":{{"round":{round}}},"player":{{"steamid":"{me}","state":{{"round_kills":{kills}}}}}}}"#
        )
    }

    fn dota_body(kills: u32) -> String {
        format!(
            r#"{{"provider":{{"appid":570}},"map":{{"matchid":"77"}},"player":{{"kills":{kills}}}}}"#
        )
    }

    #[test]
    fn kills_from_cs2_and_dota_both_arrive_over_the_one_port() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel::<(Game, u32)>();
        spawn_state_feed(port, tx, shared.clone());
        std::thread::sleep(Duration::from_millis(200));
        for body in [
            cs2_body(7, 0),
            cs2_body(7, 1),
            cs2_body(7, 2),
            dota_body(4),
            dota_body(5),
            dota_body(6),
        ] {
            send(port, &body);
        }
        let mut seen = Vec::new();
        while let Ok(item) = rx.recv_timeout(Duration::from_millis(300)) {
            seen.push(item);
        }
        assert_eq!(
            seen,
            vec![
                (Game::Cs2, 1),
                (Game::Cs2, 2),
                (Game::Dota2, 1),
                (Game::Dota2, 2)
            ]
        );
        assert!(shared.cs2_last_payload.load(Ordering::Relaxed) > 0);
        assert!(shared.dota_last_payload.load(Ordering::Relaxed) > 0);

        let saves = Arc::new(std::sync::Mutex::new(Vec::new()));
        let settings = EventsSettings {
            enabled: true,
            min_kills: 2,
            save_secs: 25,
            ..Default::default()
        };
        let mut engine = Engine::new(settings, Count(saves.clone()));
        let t0 = Instant::now();
        for (game, kills) in seen {
            engine.kill(game, kills, t0);
        }
        let out = engine.tick(t0 + Duration::from_secs(5)).unwrap();
        assert_eq!(out.kills, 2);
        assert_eq!(*saves.lock().unwrap(), vec![25]);
    }
}
