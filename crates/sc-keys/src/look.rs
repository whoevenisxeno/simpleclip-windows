#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    Neon,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    pub rgb: [u8; 3],
    pub a: f32,
}

const fn p(rgb: [u8; 3], a: f32) -> Paint {
    Paint { rgb, a }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub plate: Paint,
    pub cap: Paint,
    pub lip: Paint,
    pub label: Paint,
    pub edge: Paint,
    pub hot: Paint,
    pub hot_lip: Paint,
    pub hot_label: Paint,
    pub glow: Paint,
    pub glow_size: f32,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Dark, Theme::Light, Theme::Neon];

    pub fn from_name(name: &str) -> Self {
        match name {
            "Light" => Theme::Light,
            "Neon" => Theme::Neon,
            _ => Theme::Dark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::Neon => "Neon",
        }
    }

    pub fn look(self) -> Look {
        match self {
            Theme::Dark => Look {
                plate: p([12, 13, 17], 0.55),
                cap: p([46, 49, 58], 0.86),
                lip: p([24, 26, 32], 0.92),
                label: p([232, 235, 242], 1.0),
                edge: p([255, 255, 255], 0.07),
                hot: p([120, 190, 255], 1.0),
                hot_lip: p([70, 130, 220], 1.0),
                hot_label: p([10, 16, 28], 1.0),
                glow: p([120, 190, 255], 0.45),
                glow_size: 0.09,
            },
            Theme::Light => Look {
                plate: p([250, 251, 255], 0.3),
                cap: p([212, 216, 226], 0.62),
                lip: p([170, 176, 190], 0.9),
                label: p([34, 38, 48], 1.0),
                edge: p([255, 255, 255], 0.5),
                hot: p([255, 214, 64], 1.0),
                hot_lip: p([222, 168, 20], 1.0),
                hot_label: p([40, 30, 6], 1.0),
                glow: p([255, 214, 64], 0.35),
                glow_size: 0.07,
            },
            Theme::Neon => Look {
                plate: p([6, 2, 14], 0.6),
                cap: p([10, 10, 16], 0.9),
                lip: p([4, 4, 8], 0.95),
                label: p([150, 245, 255], 1.0),
                edge: p([0, 230, 255], 0.55),
                hot: p([255, 52, 200], 1.0),
                hot_lip: p([190, 20, 150], 1.0),
                hot_label: p([28, 0, 34], 1.0),
                glow: p([255, 52, 200], 0.9),
                glow_size: 0.14,
            },
        }
    }
}

pub fn brightness(c: Paint) -> f32 {
    c.rgb.iter().copied().max().unwrap_or(0) as f32 / 255.0 * c.a
}

pub fn mix(a: Paint, b: Paint, t: f32) -> Paint {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Paint {
        rgb: [
            f(a.rgb[0], b.rgb[0]),
            f(a.rgb[1], b.rgb[1]),
            f(a.rgb[2], b.rgb[2]),
        ],
        a: a.a + (b.a - a.a) * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
        let lum = |c: [u8; 3]| {
            let l = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn pressed_is_brighter_and_labels_read_well() {
        for t in Theme::ALL {
            let l = t.look();
            assert!(brightness(l.hot) > brightness(l.cap) + 0.1, "{t:?}");
            assert!(contrast(l.label.rgb, l.cap.rgb) > 4.5, "{t:?} idle");
            assert!(contrast(l.hot_label.rgb, l.hot.rgb) > 4.5, "{t:?} pressed");
            assert!(l.cap.a < 1.0 && l.plate.a < l.cap.a);
            assert_eq!(Theme::from_name(t.name()), t);
        }
        assert_eq!(Theme::from_name("Plaid"), Theme::Dark);
    }

    #[test]
    fn neon_glows_magenta_with_cyan_edges() {
        let l = Theme::Neon.look();
        assert!(l.hot.rgb[0] > 200 && l.hot.rgb[2] > 150 && l.hot.rgb[1] < 100);
        assert!(l.edge.rgb[1] > 200 && l.edge.rgb[2] > 200 && l.edge.rgb[0] < 50);
        assert!(l.cap.rgb.iter().all(|&c| c < 24));
        assert!(l.glow_size > Theme::Dark.look().glow_size);
    }

    #[test]
    fn mixing_goes_end_to_end() {
        let (a, b) = (p([0, 0, 0], 0.0), p([255, 100, 50], 1.0));
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
        assert_eq!(mix(a, b, 2.0), b);
        assert_eq!(mix(a, b, 0.5).rgb, [128, 50, 25]);
    }
}
