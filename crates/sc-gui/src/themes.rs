//! The built-in colour themes, and applying one. The default, "SimpleClip", is the
//! original look and follows Windows light and dark; the others are fixed palettes.
//! An accent colour can be laid over any theme.

use crate::theme::{self, Palette, DARK, LIGHT};
use egui::{Color32, Id};

/// One selectable theme. `pal` is `None` for the default, which follows the system.
pub struct ThemeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub pal: Option<Palette>,
}

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A dark palette from its surfaces, text shades and accent.
#[allow(clippy::too_many_arguments)]
const fn dark(
    bg: Color32,
    sidebar: Color32,
    card: Color32,
    card_hi: Color32,
    line: Color32,
    text: Color32,
    text_2: Color32,
    text_3: Color32,
    accent: Color32,
) -> Palette {
    Palette {
        dark: true,
        bg,
        sidebar,
        card,
        card_hi,
        line,
        text,
        text_2,
        text_3,
        accent,
        green: c(0x30, 0xd1, 0x58),
        red: c(0xff, 0x45, 0x3a),
        orange: c(0xff, 0x9f, 0x0a),
    }
}

/// A light palette from its surfaces, text shades and accent.
#[allow(clippy::too_many_arguments)]
const fn light(
    bg: Color32,
    sidebar: Color32,
    card: Color32,
    card_hi: Color32,
    line: Color32,
    text: Color32,
    text_2: Color32,
    text_3: Color32,
    accent: Color32,
) -> Palette {
    Palette {
        dark: false,
        bg,
        sidebar,
        card,
        card_hi,
        line,
        text,
        text_2,
        text_3,
        accent,
        green: c(0x34, 0xc7, 0x59),
        red: c(0xff, 0x3b, 0x30),
        orange: c(0xff, 0x95, 0x00),
    }
}

pub const THEMES: &[ThemeDef] = &[
    ThemeDef {
        id: "system",
        name: "SimpleClip",
        pal: None,
    },
    ThemeDef {
        id: "midnight",
        name: "Midnight",
        pal: Some(dark(
            c(0x0d, 0x12, 0x20),
            c(0x12, 0x1a, 0x2c),
            c(0x18, 0x22, 0x38),
            c(0x22, 0x30, 0x50),
            c(0x2a, 0x3a, 0x5c),
            c(0xea, 0xf0, 0xff),
            c(0x93, 0xa0, 0xbf),
            c(0x62, 0x70, 0x8f),
            c(0x4c, 0x8d, 0xff),
        )),
    },
    ThemeDef {
        id: "graphite",
        name: "Graphite",
        pal: Some(dark(
            c(0x15, 0x15, 0x15),
            c(0x1c, 0x1c, 0x1c),
            c(0x23, 0x23, 0x23),
            c(0x2e, 0x2e, 0x2e),
            c(0x34, 0x34, 0x34),
            c(0xf0, 0xf0, 0xf0),
            c(0x9a, 0x9a, 0x9a),
            c(0x6a, 0x6a, 0x6a),
            c(0x26, 0xa6, 0x9a),
        )),
    },
    ThemeDef {
        id: "ember",
        name: "Ember",
        pal: Some(dark(
            c(0x1a, 0x12, 0x10),
            c(0x22, 0x18, 0x15),
            c(0x2b, 0x1f, 0x1b),
            c(0x3a, 0x2a, 0x24),
            c(0x4a, 0x35, 0x2d),
            c(0xfb, 0xf1, 0xec),
            c(0xb3, 0x9a, 0x8f),
            c(0x7d, 0x66, 0x5c),
            c(0xf2, 0x6a, 0x1f),
        )),
    },
    ThemeDef {
        id: "forest",
        name: "Forest",
        pal: Some(dark(
            c(0x0f, 0x17, 0x13),
            c(0x14, 0x20, 0x1a),
            c(0x1a, 0x2a, 0x22),
            c(0x24, 0x3a, 0x2f),
            c(0x2f, 0x4a, 0x3c),
            c(0xea, 0xf5, 0xee),
            c(0x8f, 0xb0, 0x9b),
            c(0x5f, 0x80, 0x70),
            c(0x24, 0xa8, 0x62),
        )),
    },
    ThemeDef {
        id: "violet",
        name: "Violet",
        pal: Some(dark(
            c(0x14, 0x10, 0x1f),
            c(0x1a, 0x15, 0x29),
            c(0x22, 0x1b, 0x36),
            c(0x2e, 0x25, 0x48),
            c(0x3b, 0x30, 0x59),
            c(0xf1, 0xec, 0xff),
            c(0xa5, 0x97, 0xc9),
            c(0x74, 0x68, 0x9a),
            c(0x8b, 0x5c, 0xf6),
        )),
    },
    ThemeDef {
        id: "rose",
        name: "Rose",
        pal: Some(dark(
            c(0x1c, 0x10, 0x15),
            c(0x24, 0x15, 0x1b),
            c(0x2e, 0x1b, 0x23),
            c(0x3d, 0x26, 0x30),
            c(0x4f, 0x31, 0x3d),
            c(0xff, 0xee, 0xf3),
            c(0xc2, 0x9a, 0xa8),
            c(0x8a, 0x65, 0x73),
            c(0xe8, 0x45, 0x76),
        )),
    },
    ThemeDef {
        id: "ocean",
        name: "Ocean",
        pal: Some(dark(
            c(0x0a, 0x1a, 0x1f),
            c(0x0f, 0x24, 0x2b),
            c(0x15, 0x30, 0x38),
            c(0x1e, 0x41, 0x4b),
            c(0x27, 0x52, 0x60),
            c(0xe6, 0xfa, 0xff),
            c(0x8c, 0xb8, 0xc4),
            c(0x5b, 0x87, 0x94),
            c(0x0b, 0x9c, 0xbd),
        )),
    },
    ThemeDef {
        id: "daylight",
        name: "Daylight",
        pal: Some(LIGHT),
    },
    ThemeDef {
        id: "sand",
        name: "Sand",
        pal: Some(light(
            c(0xf7, 0xf1, 0xe8),
            c(0xec, 0xe3, 0xd5),
            c(0xff, 0xfd, 0xf9),
            c(0xf1, 0xe9, 0xdc),
            c(0xdd, 0xd1, 0xbf),
            c(0x2b, 0x25, 0x1d),
            c(0x7a, 0x6e, 0x5e),
            c(0xaa, 0x9e, 0x8d),
            c(0xd9, 0x73, 0x1a),
        )),
    },
    ThemeDef {
        id: "mint",
        name: "Mint",
        pal: Some(light(
            c(0xf0, 0xf7, 0xf3),
            c(0xe1, 0xee, 0xe7),
            c(0xff, 0xff, 0xff),
            c(0xe8, 0xf2, 0xec),
            c(0xcf, 0xe0, 0xd6),
            c(0x14, 0x26, 0x1c),
            c(0x54, 0x70, 0x5f),
            c(0x93, 0xab, 0x9d),
            c(0x1f, 0x9d, 0x5c),
        )),
    },
];

/// Accent colours offered on top of any theme: `#rrggbb` and a name.
pub const ACCENTS: &[(&str, &str)] = &[
    ("#0a84ff", "Blue"),
    ("#8b5cf6", "Purple"),
    ("#e84576", "Pink"),
    ("#ff453a", "Red"),
    ("#f26a1f", "Orange"),
    ("#24a862", "Green"),
    ("#0b9cbd", "Teal"),
];

fn palette_id() -> Id {
    Id::new("sc-theme-palette")
}

fn accent_id() -> Id {
    Id::new("sc-theme-accent")
}

/// The accent laid over the system theme, if one is chosen.
pub fn system_accent(ctx: &egui::Context) -> Option<Color32> {
    ctx.data(|d| d.get_temp::<Color32>(accent_id()))
}

/// The palette of the fixed theme in use, or `None` when following the system.
pub fn fixed_palette(ctx: &egui::Context) -> Option<Palette> {
    ctx.data(|d| d.get_temp::<Option<Palette>>(palette_id()))
        .flatten()
}

/// `#rrggbb` as a colour.
pub fn parse_hex(hex: &str) -> Option<Color32> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

/// Switch to theme `id` (an unknown id means the default) with an optional accent.
pub fn set_theme(ctx: &egui::Context, id: &str, accent: Option<&str>) {
    let def = THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0]);
    let accent = accent.and_then(parse_hex);
    match def.pal {
        None => {
            // The original look: Windows decides light or dark.
            let (mut d, mut l) = (DARK, LIGHT);
            if let Some(a) = accent {
                d.accent = a;
                l.accent = a;
            }
            ctx.style_mut_of(egui::Theme::Dark, |s| theme::style(s, d));
            ctx.style_mut_of(egui::Theme::Light, |s| theme::style(s, l));
            ctx.data_mut(|m| {
                m.insert_temp(palette_id(), None::<Palette>);
                match accent {
                    Some(a) => m.insert_temp(accent_id(), a),
                    None => {
                        m.remove_temp::<Color32>(accent_id());
                    }
                }
            });
            ctx.set_theme(egui::ThemePreference::System);
        }
        Some(mut p) => {
            if let Some(a) = accent {
                p.accent = a;
            }
            ctx.style_mut_of(egui::Theme::Dark, |s| theme::style(s, p));
            ctx.style_mut_of(egui::Theme::Light, |s| theme::style(s, p));
            ctx.data_mut(|m| {
                m.insert_temp(palette_id(), Some(p));
                m.remove_temp::<Color32>(accent_id());
            });
            ctx.set_theme(if p.dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
        }
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_the_default_comes_first() {
        assert_eq!(THEMES[0].id, "system");
        let mut ids: Vec<_> = THEMES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), THEMES.len());
        assert!(THEMES.len() >= 10);
    }

    #[test]
    fn hex_colours_parse() {
        assert_eq!(
            parse_hex("#0a84ff"),
            Some(Color32::from_rgb(0x0a, 0x84, 0xff))
        );
        assert_eq!(parse_hex("nope"), None);
        for (hex, _) in ACCENTS {
            assert!(parse_hex(hex).is_some());
        }
    }

    #[test]
    fn text_stays_readable_on_every_theme() {
        // A simple contrast check of body text against the page background.
        fn lum(c: Color32) -> f32 {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        }
        for t in THEMES {
            let p = t.pal.unwrap_or(DARK);
            let (a, b) = (lum(p.text), lum(p.bg));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(ratio > 7.0, "{} text contrast {ratio:.1}", t.name);
            let (a, b) = (lum(p.text_2), lum(p.card));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(ratio > 3.0, "{} secondary text contrast {ratio:.1}", t.name);
        }
    }
}
