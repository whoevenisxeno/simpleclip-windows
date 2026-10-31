use crate::state::Daemon;
use sc_core::config::Config;
use sc_core::store::{self, Catalog, Entry, Manifest};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

const CHECK_EVERY: Duration = Duration::from_secs(3);
const QUICK_DEATH: Duration = Duration::from_secs(10);
const COOL_OFF: Duration = Duration::from_secs(60);
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_UPDATES: Duration = Duration::from_secs(30 * 60);
const MAX_TRIES: u32 = 5;
const EVENTS: &str = "events";

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Nothing,
}

pub fn decide(wanted: bool, installed: bool, running: bool, cooling_off: bool) -> Action {
    match (wanted && installed, running) {
        (true, false) if !cooling_off => Action::Start,
        (false, true) => Action::Stop,
        _ => Action::Nothing,
    }
}

pub fn wanted(m: &Manifest, settings: &Map<String, Value>, config: &Config) -> bool {
    if m.name == EVENTS {
        config.events.enabled
    } else {
        m.background && store::is_enabled(settings)
    }
}

pub fn to_stage(have: &[Manifest], catalog: &Catalog, app: &str) -> Vec<Entry> {
    have.iter()
        .filter_map(|m| {
            let e = catalog.addons.iter().find(|e| e.id == m.name)?;
            (store::needs_update(m, e) && e.fits(app)).then(|| e.clone())
        })
        .collect()
}

pub fn staged(root: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let id = name.strip_suffix(".new")?;
            if !store::valid_id(id) {
                return None;
            }
            let m = store::read_manifest(&store::staged_of(root, id))?;
            (m.name == id).then(|| (id.to_string(), m.version))
        })
        .collect();
    out.sort();
    out
}

pub fn apply_staged(
    ids: &[(String, String)],
    tries: &mut HashMap<String, (String, u32)>,
    mut stop: impl FnMut(&str),
    mut apply: impl FnMut(&str) -> Result<bool, String>,
) -> Vec<String> {
    tries.retain(|id, _| ids.iter().any(|(i, _)| i == id));
    let mut held = Vec::new();
    for (id, version) in ids {
        let t = tries.entry(id.clone()).or_insert((version.clone(), 0));
        if &t.0 != version {
            *t = (version.clone(), 0);
        }
        if t.1 >= MAX_TRIES {
            continue;
        }
        stop(id);
        match apply(id) {
            Ok(_) => {
                tries.remove(id);
                tracing::info!(id = %id, version = %version, "add-on updated");
            }
            Err(e) => {
                t.1 += 1;
                if t.1 >= MAX_TRIES {
                    tracing::warn!(id = %id, error = %e, "giving up on an add-on update");
                } else {
                    tracing::warn!(id = %id, error = %e, "add-on update failed, retrying");
                    held.push(id.clone());
                }
            }
        }
    }
    held
}

#[derive(Default)]
struct Proc {
    child: Option<Child>,
    started: Option<Instant>,
    cool_until: Option<Instant>,
}

impl Proc {
    fn running(&mut self, id: &str) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            _ => {
                self.child = None;
                if self.started.is_some_and(|s| s.elapsed() < QUICK_DEATH) {
                    self.cool_until = Some(Instant::now() + COOL_OFF);
                    tracing::warn!(id, "add-on stopped right away, waiting a minute");
                }
                false
            }
        }
    }

    fn cooling(&self) -> bool {
        self.cool_until.is_some_and(|t| Instant::now() < t)
    }

    fn start(&mut self, id: &str, dir: &Path, exe: &str) {
        let mut cmd = Command::new(dir.join(exe));
        cmd.arg("--parent")
            .arg(std::process::id().to_string())
            .arg("--dir")
            .arg(dir)
            .current_dir(dir)
            .creation_flags(0x0800_0000);
        match cmd.spawn() {
            Ok(child) => {
                tracing::info!(id, pid = child.id(), "add-on started");
                self.child = Some(child);
                self.started = Some(Instant::now());
            }
            Err(e) => {
                tracing::warn!(id, error = %e, "could not start the add-on");
                self.cool_until = Some(Instant::now() + COOL_OFF);
            }
        }
    }

    fn stop(&mut self, id: &str) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!(id, "add-on stopped");
        }
    }
}

fn guard(lock: &Mutex<()>) -> Option<MutexGuard<'_, ()>> {
    match lock.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

#[derive(Default)]
struct Supervisor {
    procs: HashMap<String, Proc>,
    tries: HashMap<String, (String, u32)>,
}

impl Supervisor {
    fn tick(&mut self, root: &Path, config: &Config, lock: &Mutex<()>) {
        let mut held = Vec::new();
        if let Some(_g) = guard(lock) {
            let ids = staged(root);
            let procs = &mut self.procs;
            held = apply_staged(
                &ids,
                &mut self.tries,
                |id| {
                    if let Some(p) = procs.get_mut(id) {
                        p.stop(id);
                    }
                },
                |id| store::apply(root, id),
            );
        }
        let live = store::installed(root);
        for (dir, m) in &live {
            let want = wanted(m, &store::read_settings(dir), config) && !held.contains(&m.name);
            let p = self.procs.entry(m.name.clone()).or_default();
            let running = p.running(&m.name);
            match decide(want, true, running, p.cooling()) {
                Action::Start => p.start(&m.name, dir, &m.exe_name()),
                Action::Stop => p.stop(&m.name),
                Action::Nothing => {}
            }
        }
        let known = |id: &str| live.iter().any(|(_, m)| m.name == id);
        for (id, p) in self.procs.iter_mut() {
            if !known(id) {
                let running = p.running(id);
                if decide(false, false, running, p.cooling()) == Action::Stop {
                    p.stop(id);
                }
            }
        }
        self.procs.retain(|id, p| known(id) || p.cooling());
    }
}

fn update(lock: &Mutex<()>) {
    let Some(root) = store::root() else {
        return;
    };
    let have: Vec<Manifest> = store::installed(&root)
        .into_iter()
        .map(|(_, m)| m)
        .collect();
    if have.is_empty() {
        return;
    }
    let catalog = match store::fetch_catalog() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "could not check for add-on updates");
            return;
        }
    };
    for e in to_stage(&have, &catalog, env!("CARGO_PKG_VERSION")) {
        let ready = store::read_manifest(&store::staged_of(&root, &e.id))
            .is_some_and(|m| m.version == e.version);
        if ready {
            continue;
        }
        let _g = lock.lock().unwrap_or_else(|p| p.into_inner());
        match store::stage(&root, &e) {
            Ok(_) => tracing::info!(id = %e.id, version = %e.version, "add-on update downloaded"),
            Err(err) => {
                tracing::warn!(id = %e.id, error = %err, "could not download an add-on update")
            }
        }
    }
}

pub fn spawn(daemon: Daemon) {
    let lock = Arc::new(Mutex::new(()));
    let tick_lock = lock.clone();
    let tick_daemon = daemon.clone();
    std::thread::Builder::new()
        .name("sc-addons".into())
        .spawn(move || {
            let mut supervisor = Supervisor::default();
            loop {
                let config = tick_daemon.config.read().unwrap().clone();
                if let Some(root) = store::root() {
                    supervisor.tick(&root, &config, &tick_lock);
                }
                std::thread::sleep(CHECK_EVERY);
            }
        })
        .ok();
    std::thread::Builder::new()
        .name("sc-addon-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                if daemon.config.read().unwrap().addons.auto_update {
                    update(&lock);
                }
                std::thread::sleep(CHECK_UPDATES);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn manifest(name: &str, version: &str, exe: &str, background: bool) -> Manifest {
        Manifest {
            name: name.into(),
            version: version.into(),
            exe: exe.into(),
            background,
            ..Default::default()
        }
    }

    fn entry(id: &str, version: &str, min_app: &str) -> Entry {
        Entry {
            id: id.into(),
            version: version.into(),
            min_app: min_app.into(),
            ..Default::default()
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-addons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(dir: &Path, manifest: &str, exe: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(store::MANIFEST), manifest).unwrap();
        std::fs::write(dir.join(exe), b"x").unwrap();
    }

    #[test]
    fn it_starts_only_when_wanted_installed_and_not_already_running() {
        assert_eq!(decide(true, true, false, false), Action::Start);
        assert_eq!(decide(true, true, true, false), Action::Nothing);
        assert_eq!(decide(false, true, false, false), Action::Nothing);
        assert_eq!(decide(true, false, false, false), Action::Nothing);
    }

    #[test]
    fn it_stops_when_switched_off_or_removed() {
        assert_eq!(decide(false, true, true, false), Action::Stop);
        assert_eq!(decide(true, false, true, false), Action::Stop);
    }

    #[test]
    fn a_crashing_add_on_is_not_restarted_in_a_loop() {
        assert_eq!(decide(true, true, false, true), Action::Nothing);
        assert_eq!(decide(false, true, true, true), Action::Stop);
    }

    #[test]
    fn events_follows_the_config_and_others_follow_their_own_settings() {
        let mut config = Config::default();
        let mut off = Map::new();
        off.insert(store::ENABLED.into(), Value::Bool(false));
        let none = Map::new();
        let legacy = manifest("events", "6.9.0", "", false);
        let events = manifest("events", "1.0.0", "sc-events.exe", true);
        let voice = manifest("voice", "1.0.0", "v.exe", true);
        let tool = manifest("tool", "1.0.0", "t.exe", false);
        config.events.enabled = true;
        assert!(wanted(&legacy, &off, &config));
        assert!(wanted(&events, &none, &config));
        config.events.enabled = false;
        assert!(!wanted(&legacy, &none, &config));
        assert!(!wanted(&events, &none, &config));
        assert!(wanted(&voice, &none, &config));
        assert!(!wanted(&voice, &off, &config));
        assert!(!wanted(&tool, &none, &config));
    }

    #[test]
    fn only_outdated_add_ons_that_fit_this_app_are_picked_for_update() {
        let have = vec![
            manifest("events", "6.9.0", "", false),
            manifest("voice", "1.0.0", "v.exe", true),
            manifest("tool", "2.0.0", "t.exe", false),
            manifest("big", "1.0.0", "b.exe", true),
            manifest("lone", "1.0.0", "l.exe", true),
        ];
        let catalog = Catalog {
            addons: vec![
                entry("events", "1.0.0", ""),
                entry("voice", "1.1.0", "1.0.0"),
                entry("tool", "2.0.0", ""),
                entry("big", "2.0.0", "999.0.0"),
                entry("other", "1.0.0", ""),
            ],
        };
        let ids: Vec<String> = to_stage(&have, &catalog, "6.9.0")
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, vec!["events", "voice"]);
    }

    #[test]
    fn a_staged_update_stops_the_add_on_first_and_gives_up_after_a_few_failures() {
        let log = RefCell::new(Vec::new());
        let mut tries = HashMap::new();
        let ids = vec![
            ("bad".to_string(), "2.0.0".to_string()),
            ("good".to_string(), "1.1.0".to_string()),
        ];
        let run = |tries: &mut HashMap<String, (String, u32)>, ids: &[(String, String)]| {
            apply_staged(
                ids,
                tries,
                |id| log.borrow_mut().push(format!("stop {id}")),
                |id| {
                    log.borrow_mut().push(format!("apply {id}"));
                    if id == "bad" {
                        Err("busy".into())
                    } else {
                        Ok(true)
                    }
                },
            )
        };
        assert_eq!(run(&mut tries, &ids), vec!["bad"]);
        assert_eq!(
            *log.borrow(),
            vec!["stop bad", "apply bad", "stop good", "apply good"]
        );
        let only_bad = &ids[..1];
        for _ in 1..MAX_TRIES - 1 {
            assert_eq!(run(&mut tries, only_bad), vec!["bad"]);
        }
        assert!(run(&mut tries, only_bad).is_empty());
        log.borrow_mut().clear();
        assert!(run(&mut tries, only_bad).is_empty());
        assert!(log.borrow().is_empty());
        let newer = vec![("bad".to_string(), "2.0.1".to_string())];
        assert_eq!(run(&mut tries, &newer), vec!["bad"]);
        assert_eq!(*log.borrow(), vec!["stop bad", "apply bad"]);
    }

    #[test]
    fn staged_folders_are_found_and_swapped_in_after_the_stop() {
        let root = temp("staged");
        put(
            &root.join("voice"),
            r#"{"name":"voice","version":"1.0.0","exe":"v.exe"}"#,
            "v.exe",
        );
        put(
            &store::staged_of(&root, "voice"),
            r#"{"name":"voice","version":"1.1.0","exe":"v.exe"}"#,
            "v.exe",
        );
        put(
            &store::staged_of(&root, "odd"),
            r#"{"name":"voice","version":"9.0.0","exe":"v.exe"}"#,
            "v.exe",
        );
        let ids = staged(&root);
        assert_eq!(ids, vec![("voice".to_string(), "1.1.0".to_string())]);
        let stopped = RefCell::new(false);
        let held = apply_staged(
            &ids,
            &mut HashMap::new(),
            |_| *stopped.borrow_mut() = true,
            |id| {
                assert!(*stopped.borrow());
                store::apply(&root, id)
            },
        );
        assert!(held.is_empty());
        let m = store::read_manifest(&root.join("voice")).unwrap();
        assert_eq!(m.version, "1.1.0");
        assert!(staged(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_program_that_dies_at_once_triggers_a_cool_off_and_a_real_one_does_not() {
        let mut s = Proc::default();
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/C", "exit"]).creation_flags(0x0800_0000);
        s.child = cmd.spawn().ok();
        s.started = Some(Instant::now());
        while s
            .child
            .as_mut()
            .is_some_and(|c| c.try_wait().unwrap().is_none())
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!s.running("x"));
        assert!(s.cool_until.is_some());

        let mut s = Proc::default();
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000);
        s.child = cmd.spawn().ok();
        s.started = Some(Instant::now());
        assert!(s.running("x"));
        s.stop("x");
        assert!(!s.running("x"));
        assert!(s.cool_until.is_none());
    }
}
