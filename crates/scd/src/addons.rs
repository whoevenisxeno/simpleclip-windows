use crate::state::Daemon;
use sc_core::addon::{events_dir, EVENTS_EXE};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const CHECK_EVERY: Duration = Duration::from_secs(3);
const QUICK_DEATH: Duration = Duration::from_secs(10);
const COOL_OFF: Duration = Duration::from_secs(60);

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

struct Supervisor {
    child: Option<Child>,
    started: Option<Instant>,
    cool_until: Option<Instant>,
}

impl Supervisor {
    fn new() -> Self {
        Self {
            child: None,
            started: None,
            cool_until: None,
        }
    }

    fn running(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            _ => {
                self.child = None;
                if self.started.is_some_and(|s| s.elapsed() < QUICK_DEATH) {
                    self.cool_until = Some(Instant::now() + COOL_OFF);
                    tracing::warn!("the game events add-on stopped right away, waiting a minute");
                }
                false
            }
        }
    }

    fn start(&mut self, exe: &PathBuf) {
        let mut cmd = Command::new(exe);
        cmd.arg("--parent")
            .arg(std::process::id().to_string())
            .creation_flags(0x0800_0000);
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        match cmd.spawn() {
            Ok(child) => {
                tracing::info!(pid = child.id(), "game events add-on started");
                self.child = Some(child);
                self.started = Some(Instant::now());
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not start the game events add-on");
                self.cool_until = Some(Instant::now() + COOL_OFF);
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!("game events add-on stopped");
        }
    }

    fn tick(&mut self, wanted: bool, exe: Option<PathBuf>) {
        let running = self.running();
        let cooling = self.cool_until.is_some_and(|t| Instant::now() < t);
        match decide(wanted, exe.is_some(), running, cooling) {
            Action::Start => {
                if let Some(exe) = exe {
                    self.start(&exe);
                }
            }
            Action::Stop => self.stop(),
            Action::Nothing => {}
        }
    }
}

pub fn spawn(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-addons".into())
        .spawn(move || {
            let mut supervisor = Supervisor::new();
            loop {
                let wanted = daemon.config.read().unwrap().events.enabled;
                let exe = events_dir()
                    .map(|d| d.join(EVENTS_EXE))
                    .filter(|p| p.exists());
                supervisor.tick(wanted, exe);
                std::thread::sleep(CHECK_EVERY);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_program_that_dies_at_once_triggers_a_cool_off_and_a_real_one_does_not() {
        let mut s = Supervisor::new();
        let exe = PathBuf::from("cmd.exe");
        let mut cmd = Command::new(&exe);
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
        assert!(!s.running());
        assert!(s.cool_until.is_some());

        let mut s = Supervisor::new();
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "30", "127.0.0.1"])
            .creation_flags(0x0800_0000);
        s.child = cmd.spawn().ok();
        s.started = Some(Instant::now());
        assert!(s.running());
        s.stop();
        assert!(!s.running());
        assert!(s.cool_until.is_none());
    }
}
