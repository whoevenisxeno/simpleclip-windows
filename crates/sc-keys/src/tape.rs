use crate::draw::Lit;
use crate::layout::Board;
use std::collections::VecDeque;
use std::sync::Mutex;

pub static TAPE: Mutex<Tape> = Mutex::new(Tape::new());

pub fn record(e: Event) {
    if let Ok(mut t) = TAPE.lock() {
        t.push(e);
    }
}

pub fn recorded(from: f64, to: f64) -> Vec<Event> {
    TAPE.lock().map(|t| t.between(from, to)).unwrap_or_default()
}

pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub const FADE: f64 = 0.17;
pub const FLASH: f64 = 0.32;
pub const KEEP: f64 = 30.0 * 60.0;
const LOOK_BACK: f64 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Key(u8),
    Button(u8),
    Wheel(bool),
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub at: f64,
    pub input: Input,
    pub down: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub down: [bool; 256],
    pub gone: [Option<f64>; 256],
    pub buttons: [bool; 3],
    pub let_go: [Option<f64>; 3],
    pub scroll: Option<(f64, bool)>,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub const fn new() -> Self {
        State {
            down: [false; 256],
            gone: [None; 256],
            buttons: [false; 3],
            let_go: [None; 3],
            scroll: None,
        }
    }

    pub fn apply(&mut self, e: &Event) -> bool {
        match e.input {
            Input::Key(vk) => {
                let i = vk as usize;
                if self.down[i] == e.down {
                    return false;
                }
                self.down[i] = e.down;
                self.gone[i] = (!e.down).then_some(e.at);
            }
            Input::Button(b) => {
                let i = (b as usize).min(2);
                if self.buttons[i] == e.down {
                    return false;
                }
                self.buttons[i] = e.down;
                self.let_go[i] = (!e.down).then_some(e.at);
            }
            Input::Wheel(up) => self.scroll = Some((e.at, up)),
            Input::Clear => {
                if *self == State::new() {
                    return false;
                }
                *self = State::new();
            }
        }
        true
    }

    pub fn held(&self) -> bool {
        self.down.iter().any(|d| *d) || self.buttons.iter().any(|d| *d)
    }

    pub fn lit(&self, b: &Board, now: f64) -> (Lit, bool) {
        let mut busy = false;
        let mut fade = |down: bool, gone: Option<f64>, span: f64| -> f32 {
            if down {
                return 1.0;
            }
            let Some(at) = gone else { return 0.0 };
            let p = ((now - at) / span).max(0.0);
            if p >= 1.0 {
                return 0.0;
            }
            busy = true;
            let q = (1.0 - p) as f32;
            q * q * (3.0 - 2.0 * q)
        };
        let keys = b
            .caps
            .iter()
            .map(|c| {
                c.vks
                    .iter()
                    .map(|&v| fade(self.down[v as usize], self.gone[v as usize], FADE))
                    .fold(0.0, f32::max)
            })
            .collect();
        let mouse = [0, 1, 2].map(|i| fade(self.buttons[i], self.let_go[i], FADE));
        let (scroll, up) = match self.scroll {
            Some((at, up)) => (fade(false, Some(at), FLASH), up),
            None => (0.0, false),
        };
        (
            Lit {
                keys,
                mouse,
                scroll,
                up,
            },
            busy,
        )
    }
}

#[derive(Debug, Default)]
pub struct Tape {
    events: VecDeque<Event>,
}

impl Tape {
    pub const fn new() -> Self {
        Tape {
            events: VecDeque::new(),
        }
    }

    pub fn push(&mut self, e: Event) {
        self.events.push_back(e);
        while self.events.front().is_some_and(|f| e.at - f.at > KEEP) {
            self.events.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn between(&self, from: f64, to: f64) -> Vec<Event> {
        self.events
            .iter()
            .filter(|e| e.at >= from - LOOK_BACK && e.at <= to)
            .copied()
            .collect()
    }
}

pub fn encode(events: &[Event]) -> String {
    events
        .iter()
        .map(|e| {
            let (kind, code) = match e.input {
                Input::Key(v) => ('k', v),
                Input::Button(b) => ('b', b),
                Input::Wheel(up) => ('w', up as u8),
                Input::Clear => ('c', 0),
            };
            format!("{:.4} {kind} {code} {}\n", e.at, e.down as u8)
        })
        .collect()
}

pub fn decode(text: &str) -> Vec<Event> {
    text.lines()
        .filter_map(|l| {
            let mut p = l.split(' ');
            let at: f64 = p.next()?.parse().ok()?;
            let kind = p.next()?;
            let code: u8 = p.next()?.parse().ok()?;
            let down = p.next()? == "1";
            let input = match kind {
                "k" => Input::Key(code),
                "b" => Input::Button(code.min(2)),
                "w" => Input::Wheel(code == 1),
                "c" => Input::Clear,
                _ => return None,
            };
            Some(Event { at, input, down })
        })
        .collect()
}

pub fn pressed_during(events: &[Event], from: f64, to: f64) -> bool {
    let mut s = State::new();
    for e in events {
        if e.at > to {
            break;
        }
        if e.at >= from - FLASH && !matches!(e.input, Input::Clear) {
            return true;
        }
        s.apply(e);
    }
    s.held()
}

pub struct Player<'a> {
    events: &'a [Event],
    next: usize,
    state: State,
}

impl<'a> Player<'a> {
    pub fn new(events: &'a [Event]) -> Self {
        Player {
            events,
            next: 0,
            state: State::new(),
        }
    }

    pub fn at(&mut self, b: &Board, t: f64) -> Lit {
        if self.next > 0 && self.events[self.next - 1].at > t {
            self.next = 0;
            self.state = State::new();
        }
        while let Some(e) = self.events.get(self.next) {
            if e.at > t {
                break;
            }
            self.state.apply(e);
            self.next += 1;
        }
        self.state.lit(b, t).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;

    fn ev(at: f64, input: Input, down: bool) -> Event {
        Event { at, input, down }
    }

    fn w(b: &Board) -> usize {
        b.caps.iter().position(|c| c.vks.contains(&b'W')).unwrap()
    }

    #[test]
    fn a_key_lights_while_held_and_fades_after() {
        let b = Board::new(Layout::Wasd, true);
        let events = [
            ev(10.0, Input::Key(b'W'), true),
            ev(11.0, Input::Key(b'W'), false),
        ];
        let mut p = Player::new(&events);
        let i = w(&b);
        assert_eq!(p.at(&b, 9.9).keys[i], 0.0);
        assert_eq!(p.at(&b, 10.5).keys[i], 1.0);
        let fading = p.at(&b, 11.0 + FADE / 2.0).keys[i];
        assert!(fading > 0.1 && fading < 0.9, "{fading}");
        assert_eq!(p.at(&b, 11.0 + FADE + 0.01).keys[i], 0.0);
        assert_eq!(p.at(&b, 10.2).keys[i], 1.0, "going back replays");
    }

    #[test]
    fn mouse_buttons_and_the_wheel_light_too() {
        let b = Board::new(Layout::Wasd, true);
        let events = [
            ev(1.0, Input::Button(2), true),
            ev(1.5, Input::Wheel(true), false),
        ];
        let mut p = Player::new(&events);
        let lit = p.at(&b, 1.6);
        assert_eq!(lit.mouse[2], 1.0);
        assert!(lit.scroll > 0.0 && lit.up);
        assert_eq!(p.at(&b, 1.5 + FLASH + 0.01).scroll, 0.0);
    }

    #[test]
    fn clearing_lets_go_of_everything() {
        let b = Board::new(Layout::Wasd, true);
        let events = [
            ev(1.0, Input::Key(b'W'), true),
            ev(2.0, Input::Clear, false),
        ];
        let mut p = Player::new(&events);
        assert_eq!(p.at(&b, 1.5).keys[w(&b)], 1.0);
        assert_eq!(p.at(&b, 2.5).keys[w(&b)], 0.0);
    }

    #[test]
    fn a_clip_without_presses_is_left_alone() {
        let quiet = [
            ev(1.0, Input::Key(b'W'), true),
            ev(2.0, Input::Key(b'W'), false),
        ];
        assert!(!pressed_during(&quiet, 10.0, 20.0));
        assert!(pressed_during(&quiet, 1.5, 20.0));
        assert!(pressed_during(&quiet, 0.0, 1.2));
        let held = [ev(1.0, Input::Key(b'W'), true)];
        assert!(pressed_during(&held, 10.0, 20.0), "still held from before");
        let cleared = [
            ev(1.0, Input::Key(b'W'), true),
            ev(5.0, Input::Clear, false),
        ];
        assert!(!pressed_during(&cleared, 10.0, 20.0));
    }

    #[test]
    fn events_survive_being_written_down_and_read_back() {
        let events = [
            ev(1_700_000_000.125, Input::Key(b'W'), true),
            ev(1_700_000_000.5, Input::Button(2), false),
            ev(1_700_000_001.0, Input::Wheel(true), false),
            ev(1_700_000_002.0, Input::Clear, false),
        ];
        let back = decode(&encode(&events));
        assert_eq!(back, events);
        assert!(decode("nonsense\n1.0 z 3 1\n").is_empty());
    }

    #[test]
    fn the_tape_forgets_old_presses_and_cuts_a_window() {
        let mut t = Tape::new();
        for i in 0..10 {
            t.push(ev(i as f64 * 600.0, Input::Key(b'W'), i % 2 == 0));
        }
        assert!(t.len() <= (KEEP / 600.0) as usize + 1);
        let part = t.between(4800.0, 5000.0);
        assert!(part
            .iter()
            .all(|e| e.at >= 4800.0 - LOOK_BACK && e.at <= 5000.0));
        assert_eq!(part.len(), 1);
    }
}
