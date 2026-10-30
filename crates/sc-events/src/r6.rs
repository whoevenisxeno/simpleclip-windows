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
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub words: Vec<Word>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub kills: u32,
    pub team_kills: u32,
    pub deaths: u32,
}

pub struct Planes<'a> {
    pub tint: &'a [u8],
    pub width: u32,
    pub height: u32,
}

const BAND: (f32, f32) = (0.69, 0.80);
const CHROMA: i32 = 12;
const MIN_PIXELS: usize = 30;
const FRIENDLY_FROM: f32 = 90.0;
const FRIENDLY_BELOW: f32 = 108.0;
const ENEMY_FROM: f32 = 130.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Enemy,
    Friendly,
    Unsure,
}

pub fn victim_tint(planes: &Planes, you: &Word) -> Option<f32> {
    let (w, h) = (planes.width as usize, planes.height as usize);
    if w == 0 || h == 0 || planes.tint.len() < w * h {
        return None;
    }
    let x0 = (BAND.0 * w as f32) as usize;
    let x1 = ((BAND.1 * w as f32) as usize).min(w);
    let y0 = you.y.max(0.0) as usize;
    let y1 = ((you.y + you.h.max(1.0)) as usize).min(h);
    let (mut sum, mut count) = (0u32, 0usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let t = planes.tint[y * w + x] as i32;
            if (t - 128).abs() >= CHROMA {
                sum += t as u32;
                count += 1;
            }
        }
    }
    (count >= MIN_PIXELS).then(|| sum as f32 / count as f32)
}

pub fn victim_side(planes: &Planes, you: &Word) -> Side {
    match victim_tint(planes, you) {
        Some(t) if t >= ENEMY_FROM => Side::Enemy,
        Some(t) if (FRIENDLY_FROM..FRIENDLY_BELOW).contains(&t) => Side::Friendly,
        _ => Side::Unsure,
    }
}

pub fn is_you(text: &str) -> bool {
    let letters: String = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    matches!(letters.as_str(), "you" | "yoo" | "y0u" | "yov" | "yau")
}

pub fn read_feed(lines: &[Line], width: f32, planes: Option<&Planes>) -> Reading {
    let mut reading = Reading::default();
    for word in lines.iter().flat_map(|l| &l.words) {
        if !is_you(&word.text) {
            continue;
        }
        if word.x / width < KILLER_SIDE {
            match planes.map_or(Side::Enemy, |p| victim_side(p, word)) {
                Side::Enemy => reading.kills += 1,
                Side::Friendly => reading.team_kills += 1,
                Side::Unsure => {}
            }
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
        let after_a_dropout = self
            .dropped
            .filter(|(_, at)| now.duration_since(*at) < DROPOUT)
            .map(|(was, _)| was);
        let already_counted = after_a_dropout.map_or(self.shown, |was| was.max(self.shown));
        self.shown = on_screen;
        let fresh = on_screen.saturating_sub(already_counted);
        if fresh == 0 {
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
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
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
            read_feed(&lines, WIDTH, None),
            Reading {
                kills: 1,
                team_kills: 0,
                deaths: 0
            }
        );
    }

    #[test]
    fn you_at_the_right_of_a_line_is_my_death() {
        let lines = [line(&[("DrUlEs79", 172.0), ("G)", 336.0), ("YOU", 367.0)])];
        assert_eq!(
            read_feed(&lines, WIDTH, None),
            Reading {
                kills: 0,
                team_kills: 0,
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
            read_feed(&lines, WIDTH, None),
            Reading {
                kills: 1,
                team_kills: 0,
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
        assert_eq!(read_feed(&lines, WIDTH, None), Reading::default());
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
    fn a_dropout_followed_by_a_new_kill_counts_only_the_new_one() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        assert_eq!(t.feed(1, ms(0)), Some(1));
        assert_eq!(t.feed(1, ms(400)), None);
        assert_eq!(t.feed(0, ms(800)), None);
        assert_eq!(t.feed(2, ms(1200)), Some(2));
        assert_eq!(t.feed(2, ms(1600)), None);
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

#[cfg(test)]
mod team_tests {
    use super::*;

    const W: u32 = 500;
    const H: u32 = 300;

    fn plane(fill: u8) -> Vec<u8> {
        vec![fill; (W * H) as usize]
    }

    fn you_at(y: f32) -> Word {
        Word {
            text: "YOU".into(),
            x: 200.0,
            y,
            w: 30.0,
            h: 12.0,
        }
    }

    fn planes(tint: &[u8]) -> Planes<'_> {
        Planes {
            tint,
            width: W,
            height: H,
        }
    }

    fn paint(tint: &mut [u8], y0: u32, value: u8) {
        for y in y0..y0 + 14 {
            for x in 345..400 {
                tint[(y * W + x) as usize] = value;
            }
        }
    }

    #[test]
    fn a_purple_victim_chip_is_a_team_kill() {
        let mut t = plane(128);
        paint(&mut t, 100, 100);
        assert_eq!(victim_side(&planes(&t), &you_at(101.0)), Side::Friendly);
    }

    #[test]
    fn an_olive_victim_chip_is_an_enemy() {
        let mut t = plane(128);
        paint(&mut t, 100, 152);
        assert_eq!(victim_side(&planes(&t), &you_at(101.0)), Side::Enemy);
    }

    #[test]
    fn a_grey_or_unreadable_area_is_never_called_a_team_kill() {
        let t = plane(128);
        assert_eq!(victim_side(&planes(&t), &you_at(101.0)), Side::Unsure);
        assert_eq!(victim_side(&planes(&[]), &you_at(101.0)), Side::Unsure);
        let mut faint = plane(128);
        paint(&mut faint, 100, 100);
        assert_eq!(victim_side(&planes(&faint), &you_at(250.0)), Side::Unsure);
    }

    #[test]
    fn a_faded_or_half_hidden_chip_is_left_out_instead_of_guessed() {
        for value in [113u8, 120, 125] {
            let mut t = plane(128);
            paint(&mut t, 100, value);
            assert_eq!(
                victim_side(&planes(&t), &you_at(101.0)),
                Side::Unsure,
                "{value}"
            );
        }
        let mut t = plane(128);
        paint(&mut t, 100, 120);
        let lines = [Line {
            words: vec![you_at(101.0)],
        }];
        let reading = read_feed(&lines, W as f32, Some(&planes(&t)));
        assert_eq!((reading.kills, reading.team_kills), (0, 0));
    }

    #[test]
    fn only_the_line_that_was_read_decides() {
        let mut t = plane(128);
        paint(&mut t, 100, 100);
        paint(&mut t, 140, 152);
        let lines = [
            Line {
                words: vec![you_at(101.0)],
            },
            Line {
                words: vec![you_at(141.0)],
            },
        ];
        let reading = read_feed(&lines, W as f32, Some(&planes(&t)));
        assert_eq!(
            reading,
            Reading {
                kills: 1,
                team_kills: 1,
                deaths: 0
            }
        );
    }

    #[test]
    fn without_a_tint_every_kill_counts_as_before() {
        let lines = [Line {
            words: vec![you_at(101.0)],
        }];
        let reading = read_feed(&lines, W as f32, None);
        assert_eq!((reading.kills, reading.team_kills), (1, 0));
    }
}
