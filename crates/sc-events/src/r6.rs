use std::time::{Duration, Instant};

pub const REGION: [f32; 4] = [0.74, 0.06, 1.0, 0.36];
pub const PROCESS: &str = "rainbowsix";
const KILLER_SIDE: f32 = 0.62;
const STREAK_WINDOW: Duration = Duration::from_secs(20);
const DROPOUT: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub words: Vec<Word>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub kills: u32,
    pub deaths: u32,
}

pub fn is_you(text: &str) -> bool {
    let letters: String = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    matches!(letters.as_str(), "you" | "yoo" | "y0u" | "yov" | "yau")
}

pub fn read_feed(lines: &[Line], width: f32) -> Reading {
    let mut reading = Reading::default();
    for word in lines.iter().flat_map(|l| &l.words) {
        if !is_you(&word.text) {
            continue;
        }
        if word.x / width < KILLER_SIDE {
            reading.kills += 1;
        } else {
            reading.deaths += 1;
        }
    }
    reading
}

#[derive(Default)]
pub struct Tracker {
    shown: u32,
    dropped: Option<(u32, Instant)>,
    recent: Vec<Instant>,
}

impl Tracker {
    pub fn feed(&mut self, on_screen: u32, now: Instant) -> Option<u32> {
        if on_screen <= self.shown {
            if on_screen < self.shown {
                self.dropped = Some((self.shown, now));
            }
            self.shown = on_screen;
            return None;
        }
        let back_from_a_dropout = self
            .dropped
            .is_some_and(|(was, at)| on_screen <= was && now.duration_since(at) < DROPOUT);
        let fresh = on_screen - self.shown;
        self.shown = on_screen;
        if back_from_a_dropout {
            return None;
        }
        self.dropped = None;
        for _ in 0..fresh {
            self.recent.push(now);
        }
        self.recent
            .retain(|t| now.duration_since(*t) <= STREAK_WINDOW);
        Some(self.recent.len() as u32)
    }
}

pub fn upscale_bgra(gray: &[u8], width: u32, height: u32, scale: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width as usize, height as usize);
    let (ow, oh) = (w * scale as usize, h * scale as usize);
    let mut out = Vec::with_capacity(ow * oh * 4);
    let at = |x: usize, y: usize| gray[y.min(h - 1) * w + x.min(w - 1)] as f32;
    for oy in 0..oh {
        let sy = ((oy as f32 + 0.5) / scale as f32 - 0.5).max(0.0);
        let (y0, fy) = (sy as usize, sy.fract());
        for ox in 0..ow {
            let sx = ((ox as f32 + 0.5) / scale as f32 - 0.5).max(0.0);
            let (x0, fx) = (sx as usize, sx.fract());
            let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
            let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
            let v = (top * (1.0 - fy) + bottom * fy).round() as u8;
            out.extend_from_slice(&[v, v, v, 255]);
        }
    }
    (out, ow as u32, oh as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(words: &[(&str, f32)]) -> Line {
        Line {
            words: words
                .iter()
                .map(|(t, x)| Word {
                    text: t.to_string(),
                    x: *x,
                })
                .collect(),
        }
    }

    const WIDTH: f32 = 499.0;

    #[test]
    fn the_word_you_is_recognised_through_common_misreads() {
        for ok in ["YOU", "you", "You", "Yoo", "Y0U", "YOU.", "(YOU)"] {
            assert!(is_you(ok), "{ok}");
        }
        for no in ["Eftovski.21", "your", "yo", "bovio", "G)", "", "?"] {
            assert!(!is_you(no), "{no}");
        }
    }

    #[test]
    fn you_at_the_left_of_a_line_is_my_kill() {
        let lines = [line(&[("YOU", 196.0), ("Kloud_388", 321.0), ("?", 419.0)])];
        assert_eq!(
            read_feed(&lines, WIDTH),
            Reading {
                kills: 1,
                deaths: 0
            }
        );
    }

    #[test]
    fn you_at_the_right_of_a_line_is_my_death() {
        let lines = [line(&[("DrUlEs79", 172.0), ("G)", 336.0), ("YOU", 367.0)])];
        assert_eq!(
            read_feed(&lines, WIDTH),
            Reading {
                kills: 0,
                deaths: 1
            }
        );
    }

    #[test]
    fn a_trade_counts_both_even_when_the_lines_are_split_up() {
        let lines = [
            line(&[("e", 336.0), ("YOU", 367.0)]),
            line(&[("YOU", 259.0), ("DrUlEs79", 333.0)]),
        ];
        assert_eq!(
            read_feed(&lines, WIDTH),
            Reading {
                kills: 1,
                deaths: 1
            }
        );
    }

    #[test]
    fn other_players_lines_and_menus_read_as_nothing() {
        let lines = [
            line(&[("bovio", 97.0), ("Eftovski.21", 242.0)]),
            line(&[("RANKED", 191.0)]),
            line(&[("2F", 49.0), ("Kids'", 72.0), ("Dorms", 110.0)]),
        ];
        assert_eq!(read_feed(&lines, WIDTH), Reading::default());
    }

    #[test]
    fn a_new_line_counts_once_while_it_stays_on_screen() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        assert_eq!(t.feed(0, ms(0)), None);
        assert_eq!(t.feed(1, ms(400)), Some(1));
        for i in 2..12 {
            assert_eq!(t.feed(1, ms(i * 400)), None);
        }
    }

    #[test]
    fn a_missed_reading_does_not_count_the_same_kill_twice() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        assert_eq!(t.feed(1, ms(0)), Some(1));
        assert_eq!(t.feed(0, ms(400)), None);
        assert_eq!(t.feed(1, ms(800)), None);
        assert_eq!(t.feed(1, ms(1200)), None);
    }

    #[test]
    fn a_kill_after_the_old_line_faded_counts_and_streaks_build_up() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        assert_eq!(t.feed(1, s(0)), Some(1));
        assert_eq!(t.feed(1, s(3)), None);
        assert_eq!(t.feed(2, s(4)), Some(2));
        assert_eq!(t.feed(0, s(10)), None);
        assert_eq!(t.feed(0, s(12)), None);
        assert_eq!(t.feed(1, s(15)), Some(3));
        assert_eq!(t.feed(0, s(25)), None);
        assert_eq!(t.feed(1, s(30)), Some(2));
        assert_eq!(t.feed(0, s(40)), None);
        assert_eq!(t.feed(0, s(42)), None);
        assert_eq!(t.feed(1, s(60)), Some(1));
    }

    #[test]
    fn two_kills_seen_at_once_count_as_two() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(2, Instant::now()), Some(2));
    }

    #[test]
    fn upscaling_keeps_the_picture_and_doubles_its_size() {
        let gray = vec![0, 255, 0, 255];
        let (out, w, h) = upscale_bgra(&gray, 2, 2, 2);
        assert_eq!((w, h), (4, 4));
        assert_eq!(out.len(), 4 * 4 * 4);
        assert_eq!(out[3], 255);
        assert_eq!(out[0], 0);
        assert_eq!(out[3 * 4], 255);
        let flat = upscale_bgra(&[90; 9], 3, 3, 3).0;
        assert!(flat.chunks(4).all(|p| p == [90, 90, 90, 255]));
    }
}
