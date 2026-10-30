use sc_core::config::EventsSettings;
use std::time::{Duration, Instant};

pub const HOLD: Duration = Duration::from_secs(4);
pub const MAX_HOLD: Duration = Duration::from_secs(14);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    Cs2,
    League,
}

impl Game {
    pub fn name(self) -> &'static str {
        match self {
            Game::Cs2 => "CS2",
            Game::League => "League of Legends",
        }
    }
}

pub trait Saver {
    fn save(&mut self, secs: u32) -> Result<(), String>;
}

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub game: Game,
    pub kills: u32,
    pub result: Result<(), String>,
}

struct Pending {
    game: Game,
    first: Instant,
    due: Instant,
    kills: u32,
}

pub struct Engine<S: Saver> {
    settings: EventsSettings,
    saver: S,
    pending: Option<Pending>,
}

impl<S: Saver> Engine<S> {
    pub fn new(settings: EventsSettings, saver: S) -> Self {
        Self {
            settings,
            saver,
            pending: None,
        }
    }

    pub fn set_settings(&mut self, settings: EventsSettings) {
        if !settings.enabled {
            self.pending = None;
        }
        self.settings = settings;
    }

    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn kill(&mut self, game: Game, kills: u32, now: Instant) {
        let s = &self.settings;
        let wanted = match game {
            Game::Cs2 => s.cs2,
            Game::League => s.league,
        };
        if !s.enabled || !wanted || kills < s.min_kills {
            return;
        }
        let first = self.pending.as_ref().map_or(now, |p| p.first);
        let kills = self.pending.as_ref().map_or(kills, |p| p.kills.max(kills));
        self.pending = Some(Pending {
            game,
            first,
            due: (now + HOLD).min(first + MAX_HOLD),
            kills,
        });
    }

    pub fn tick(&mut self, now: Instant) -> Option<Outcome> {
        if self.pending.as_ref().is_none_or(|p| now < p.due) {
            return None;
        }
        let p = self.pending.take()?;
        let result = self.saver.save(self.settings.save_secs);
        Some(Outcome {
            game: p.game,
            kills: p.kills,
            result,
        })
    }
}

pub fn describe_kills(kills: u32) -> String {
    match kills {
        0 | 1 => "Kill".to_string(),
        2 => "Double kill".to_string(),
        3 => "Triple kill".to_string(),
        4 => "Quadra kill".to_string(),
        _ => "Penta kill".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        saves: Vec<u32>,
        fail: bool,
    }

    impl Saver for &mut Recorder {
        fn save(&mut self, secs: u32) -> Result<(), String> {
            self.saves.push(secs);
            if self.fail {
                Err("capture is not running".into())
            } else {
                Ok(())
            }
        }
    }

    fn on() -> EventsSettings {
        EventsSettings {
            enabled: true,
            ..Default::default()
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_kill_saves_once_the_moment_has_played_out() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut e = Engine::new(on(), &mut rec);
        e.kill(Game::Cs2, 2, t0);
        assert!(e.tick(t0 + secs(3)).is_none());
        let out = e.tick(t0 + secs(4)).unwrap();
        assert_eq!((out.game, out.kills, out.result), (Game::Cs2, 2, Ok(())));
        assert!(e.tick(t0 + secs(60)).is_none());
        assert_eq!(rec.saves, vec![20]);
    }

    #[test]
    fn a_streak_of_kills_becomes_one_clip() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut e = Engine::new(on(), &mut rec);
        e.kill(Game::Cs2, 2, t0);
        e.kill(Game::Cs2, 3, t0 + secs(3));
        e.kill(Game::Cs2, 4, t0 + secs(6));
        assert!(e.tick(t0 + secs(9)).is_none());
        let out = e.tick(t0 + secs(10)).unwrap();
        assert_eq!(out.kills, 4);
        assert!(e.tick(t0 + secs(30)).is_none());
        assert_eq!(rec.saves.len(), 1);
    }

    #[test]
    fn a_never_ending_streak_still_saves() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut e = Engine::new(on(), &mut rec);
        for i in 0..10 {
            e.kill(Game::League, 2 + i, t0 + secs(i as u64 * 3));
        }
        let out = e.tick(t0 + MAX_HOLD).unwrap();
        assert_eq!(out.kills, 11);
    }

    #[test]
    fn kills_below_the_threshold_are_ignored() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut e = Engine::new(on(), &mut rec);
        e.kill(Game::Cs2, 1, t0);
        assert!(!e.has_pending());
        assert!(e.tick(t0 + secs(60)).is_none());
        let mut s = on();
        s.min_kills = 1;
        e.set_settings(s);
        e.kill(Game::Cs2, 1, t0);
        assert!(e.has_pending());
    }

    #[test]
    fn nothing_happens_while_the_add_on_is_off_or_the_game_is_unticked() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut e = Engine::new(EventsSettings::default(), &mut rec);
        e.kill(Game::Cs2, 5, t0);
        assert!(!e.has_pending());
        let mut s = on();
        s.league = false;
        e.set_settings(s);
        e.kill(Game::League, 5, t0);
        assert!(!e.has_pending());
        e.kill(Game::Cs2, 5, t0);
        assert!(e.has_pending());
    }

    #[test]
    fn switching_off_cancels_a_waiting_clip() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut e = Engine::new(on(), &mut rec);
        e.kill(Game::Cs2, 3, t0);
        e.set_settings(EventsSettings::default());
        assert!(e.tick(t0 + secs(30)).is_none());
        assert!(rec.saves.is_empty());
    }

    #[test]
    fn the_chosen_clip_length_is_used() {
        let mut rec = Recorder::default();
        let t0 = Instant::now();
        let mut s = on();
        s.save_secs = 45;
        let mut e = Engine::new(s, &mut rec);
        e.kill(Game::Cs2, 2, t0);
        e.tick(t0 + secs(5));
        assert_eq!(rec.saves, vec![45]);
    }

    #[test]
    fn a_failed_save_is_reported_not_retried() {
        let mut rec = Recorder {
            fail: true,
            ..Default::default()
        };
        let t0 = Instant::now();
        let mut e = Engine::new(on(), &mut rec);
        e.kill(Game::Cs2, 2, t0);
        let out = e.tick(t0 + secs(5)).unwrap();
        assert!(out.result.is_err());
        assert!(e.tick(t0 + secs(60)).is_none());
    }

    #[test]
    fn kill_counts_have_names() {
        assert_eq!(describe_kills(1), "Kill");
        assert_eq!(describe_kills(2), "Double kill");
        assert_eq!(describe_kills(3), "Triple kill");
        assert_eq!(describe_kills(4), "Quadra kill");
        assert_eq!(describe_kills(5), "Penta kill");
        assert_eq!(describe_kills(7), "Penta kill");
    }
}
