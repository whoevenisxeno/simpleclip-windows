#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Wasd,
    Arrows,
    Full,
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Wasd, Layout::Arrows, Layout::Full];

    pub fn from_name(name: &str) -> Self {
        match name {
            "Arrows" => Layout::Arrows,
            "Full keyboard" => Layout::Full,
            _ => Layout::Wasd,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Layout::Wasd => "WASD",
            Layout::Arrows => "Arrows",
            Layout::Full => "Full keyboard",
        }
    }

    pub fn unit(self) -> f32 {
        match self {
            Layout::Wasd => 58.0,
            Layout::Arrows => 52.0,
            Layout::Full => 32.0,
        }
    }
}

pub const SHIFT: &[u8] = &[0xA0, 0xA1];
pub const CTRL: &[u8] = &[0xA2, 0xA3];
pub const UP: &str = "\u{2191}";
pub const DOWN: &str = "\u{2193}";
pub const LEFT: &str = "\u{2190}";
pub const RIGHT: &str = "\u{2192}";
pub const DEMO: &[u8] = &[b'W', b'A', 0x26, 0x25];

#[derive(Debug, Clone, PartialEq)]
pub struct Cap {
    pub label: &'static str,
    pub vks: &'static [u8],
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    pub caps: Vec<Cap>,
    pub mouse: Option<Area>,
    pub w: f32,
    pub h: f32,
}

pub const MOUSE_GAP: f32 = 0.3;

impl Board {
    pub fn new(layout: Layout, mouse: bool) -> Self {
        let caps = caps(layout);
        let kw = caps.iter().map(|c| c.x + c.w).fold(0.0, f32::max);
        let h = caps.iter().map(|c| c.y + c.h).fold(0.0, f32::max);
        let mouse = mouse.then(|| {
            let w = (h * 0.5).min(2.0);
            let mh = (w * 1.75).min(h);
            Area {
                x: kw + MOUSE_GAP,
                y: (h - mh) / 2.0,
                w,
                h: mh,
            }
        });
        let w = mouse.map(|m| m.x + m.w).unwrap_or(kw);
        Board { caps, mouse, w, h }
    }

    pub fn wants(&self, vk: u8) -> bool {
        self.caps.iter().any(|c| c.vks.contains(&vk))
    }
}

struct Row {
    y: f32,
    x: f32,
    keys: &'static [(&'static str, &'static [u8], f32)],
}

fn build(rows: &[Row]) -> Vec<Cap> {
    let mut out = Vec::new();
    for r in rows {
        let mut x = r.x;
        for &(label, vks, w) in r.keys {
            if !label.is_empty() || !vks.is_empty() {
                out.push(Cap {
                    label,
                    vks,
                    x,
                    y: r.y,
                    w,
                    h: 1.0,
                });
            }
            x += w;
        }
    }
    out
}

fn caps(layout: Layout) -> Vec<Cap> {
    match layout {
        Layout::Wasd => build(&[
            Row {
                y: 0.0,
                x: 0.25,
                keys: &[
                    ("Q", b"Q", 1.0),
                    ("W", b"W", 1.0),
                    ("E", b"E", 1.0),
                    ("R", b"R", 1.0),
                ],
            },
            Row {
                y: 1.0,
                x: 0.5,
                keys: &[
                    ("A", b"A", 1.0),
                    ("S", b"S", 1.0),
                    ("D", b"D", 1.0),
                    ("F", b"F", 1.0),
                ],
            },
            Row {
                y: 2.0,
                x: 0.0,
                keys: &[
                    ("Shift", SHIFT, 1.5),
                    ("Z", b"Z", 1.0),
                    ("X", b"X", 1.0),
                    ("C", b"C", 1.0),
                ],
            },
            Row {
                y: 3.0,
                x: 0.0,
                keys: &[("Ctrl", CTRL, 1.5), ("Space", &[0x20], 3.0)],
            },
        ]),
        Layout::Arrows => build(&[
            Row {
                y: 0.0,
                x: 0.0,
                keys: &[("Enter", &[0x0D], 2.0), ("", &[], 1.25), (UP, &[0x26], 1.0)],
            },
            Row {
                y: 1.0,
                x: 0.0,
                keys: &[
                    ("Shift", SHIFT, 2.0),
                    ("", &[], 0.25),
                    (LEFT, &[0x25], 1.0),
                    (DOWN, &[0x28], 1.0),
                    (RIGHT, &[0x27], 1.0),
                ],
            },
            Row {
                y: 2.0,
                x: 0.0,
                keys: &[("Ctrl", CTRL, 1.5), ("Space", &[0x20], 3.75)],
            },
        ]),
        Layout::Full => build(&[
            Row {
                y: 0.0,
                x: 0.0,
                keys: &[
                    ("Esc", &[0x1B], 1.0),
                    ("1", b"1", 1.0),
                    ("2", b"2", 1.0),
                    ("3", b"3", 1.0),
                    ("4", b"4", 1.0),
                    ("5", b"5", 1.0),
                    ("6", b"6", 1.0),
                    ("7", b"7", 1.0),
                    ("8", b"8", 1.0),
                    ("9", b"9", 1.0),
                    ("0", b"0", 1.0),
                    ("-", &[0xBD], 1.0),
                    ("=", &[0xBB], 1.0),
                    ("Back", &[0x08], 2.0),
                ],
            },
            Row {
                y: 1.0,
                x: 0.0,
                keys: &[
                    ("Tab", &[0x09], 1.5),
                    ("Q", b"Q", 1.0),
                    ("W", b"W", 1.0),
                    ("E", b"E", 1.0),
                    ("R", b"R", 1.0),
                    ("T", b"T", 1.0),
                    ("Y", b"Y", 1.0),
                    ("U", b"U", 1.0),
                    ("I", b"I", 1.0),
                    ("O", b"O", 1.0),
                    ("P", b"P", 1.0),
                    ("[", &[0xDB], 1.0),
                    ("]", &[0xDD], 1.0),
                    ("\\", &[0xDC], 1.5),
                ],
            },
            Row {
                y: 2.0,
                x: 0.0,
                keys: &[
                    ("Caps", &[0x14], 1.75),
                    ("A", b"A", 1.0),
                    ("S", b"S", 1.0),
                    ("D", b"D", 1.0),
                    ("F", b"F", 1.0),
                    ("G", b"G", 1.0),
                    ("H", b"H", 1.0),
                    ("J", b"J", 1.0),
                    ("K", b"K", 1.0),
                    ("L", b"L", 1.0),
                    (";", &[0xBA], 1.0),
                    ("'", &[0xDE], 1.0),
                    ("Enter", &[0x0D], 2.25),
                ],
            },
            Row {
                y: 3.0,
                x: 0.0,
                keys: &[
                    ("Shift", &[0xA0], 2.25),
                    ("Z", b"Z", 1.0),
                    ("X", b"X", 1.0),
                    ("C", b"C", 1.0),
                    ("V", b"V", 1.0),
                    ("B", b"B", 1.0),
                    ("N", b"N", 1.0),
                    ("M", b"M", 1.0),
                    (",", &[0xBC], 1.0),
                    (".", &[0xBE], 1.0),
                    ("/", &[0xBF], 1.0),
                    ("Shift", &[0xA1], 2.75),
                ],
            },
            Row {
                y: 4.0,
                x: 0.0,
                keys: &[
                    ("Ctrl", &[0xA2], 1.25),
                    ("Win", &[0x5B], 1.25),
                    ("Alt", &[0xA4], 1.25),
                    ("Space", &[0x20], 6.25),
                    ("Alt", &[0xA5], 1.25),
                    ("Win", &[0x5C], 1.25),
                    ("Menu", &[0x5D], 1.25),
                    ("Ctrl", &[0xA3], 1.25),
                ],
            },
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlap(a: &Cap, b: &Cap) -> bool {
        a.x < b.x + b.w - 1e-4
            && b.x < a.x + a.w - 1e-4
            && a.y < b.y + b.h - 1e-4
            && b.y < a.y + a.h - 1e-4
    }

    #[test]
    fn no_keys_overlap_and_all_fit_inside() {
        for l in Layout::ALL {
            for mouse in [false, true] {
                let b = Board::new(l, mouse);
                assert!(!b.caps.is_empty());
                for (i, a) in b.caps.iter().enumerate() {
                    assert!(a.x >= 0.0 && a.y >= 0.0, "{:?} {}", l, a.label);
                    assert!(a.x + a.w <= b.w + 1e-4 && a.y + a.h <= b.h + 1e-4);
                    assert!(!a.vks.is_empty() && !a.label.is_empty());
                    for c in &b.caps[i + 1..] {
                        assert!(!overlap(a, c), "{:?}: {} and {}", l, a.label, c.label);
                    }
                }
                if let Some(m) = b.mouse {
                    assert!(b.caps.iter().all(|c| c.x + c.w < m.x));
                    assert!(m.x + m.w <= b.w + 1e-4 && m.y + m.h <= b.h + 1e-4);
                }
            }
        }
    }

    #[test]
    fn the_layouts_have_the_expected_keys() {
        let labels =
            |l| -> Vec<&str> { Board::new(l, false).caps.iter().map(|c| c.label).collect() };
        assert_eq!(
            labels(Layout::Wasd),
            ["Q", "W", "E", "R", "A", "S", "D", "F", "Shift", "Z", "X", "C", "Ctrl", "Space"]
        );
        let arrows = labels(Layout::Arrows);
        for l in [UP, DOWN, LEFT, RIGHT, "Space", "Enter", "Shift", "Ctrl"] {
            assert!(arrows.contains(&l), "{l}");
        }
        let full = Board::new(Layout::Full, false);
        assert_eq!(full.caps.len(), 61);
        assert!((full.w - 15.0).abs() < 1e-4 && (full.h - 5.0).abs() < 1e-4);
        assert!(full.wants(0xA1) && full.wants(b'P') && !full.wants(0x26));
        assert!(Board::new(Layout::Wasd, false).wants(0xA1));
    }

    #[test]
    fn names_round_trip() {
        for l in Layout::ALL {
            assert_eq!(Layout::from_name(l.name()), l);
        }
        assert_eq!(Layout::from_name("nope"), Layout::Wasd);
    }
}
