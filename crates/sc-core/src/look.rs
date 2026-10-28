#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub dark: bool,
    pub card: [u8; 3],
    pub line: [u8; 3],
    pub text: [u8; 3],
    pub text_2: [u8; 3],
    pub text_3: [u8; 3],
    pub accent: [u8; 3],
    pub green: [u8; 3],
    pub red: [u8; 3],
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

const fn dark(card: u32, line: u32, text: u32, text_2: u32, text_3: u32, accent: u32) -> Look {
    Look {
        dark: true,
        card: hex(card),
        line: hex(line),
        text: hex(text),
        text_2: hex(text_2),
        text_3: hex(text_3),
        accent: hex(accent),
        green: hex(0x30d158),
        red: hex(0xff453a),
    }
}

const fn light(card: u32, line: u32, text: u32, text_2: u32, text_3: u32, accent: u32) -> Look {
    Look {
        dark: false,
        card: hex(card),
        line: hex(line),
        text: hex(text),
        text_2: hex(text_2),
        text_3: hex(text_3),
        accent: hex(accent),
        green: hex(0x34c759),
        red: hex(0xff3b30),
    }
}

pub const SYSTEM_DARK: Look = dark(0x2c2c2e, 0x3d3d40, 0xf5f5f7, 0x9a9aa0, 0x787880, 0x0a84ff);
pub const SYSTEM_LIGHT: Look = light(0xffffff, 0xdcdce1, 0x1d1d1f, 0x6e6e73, 0x8e8e93, 0x007aff);

pub const THEMES: &[(&str, Look)] = &[
    (
        "midnight",
        dark(0x182238, 0x2a3a5c, 0xeaf0ff, 0x93a0bf, 0x62708f, 0x4c8dff),
    ),
    (
        "graphite",
        dark(0x232323, 0x343434, 0xf0f0f0, 0x9a9a9a, 0x6a6a6a, 0x26a69a),
    ),
    (
        "ember",
        dark(0x2b1f1b, 0x4a352d, 0xfbf1ec, 0xb39a8f, 0x7d665c, 0xf26a1f),
    ),
    (
        "forest",
        dark(0x1a2a22, 0x2f4a3c, 0xeaf5ee, 0x8fb09b, 0x5f8070, 0x24a862),
    ),
    (
        "violet",
        dark(0x221b36, 0x3b3059, 0xf1ecff, 0xa597c9, 0x74689a, 0x8b5cf6),
    ),
    (
        "rose",
        dark(0x2e1b23, 0x4f313d, 0xffeef3, 0xc29aa8, 0x8a6573, 0xe84576),
    ),
    (
        "ocean",
        dark(0x153038, 0x275260, 0xe6faff, 0x8cb8c4, 0x5b8794, 0x0b9cbd),
    ),
    ("daylight", SYSTEM_LIGHT),
    (
        "sand",
        light(0xfffdf9, 0xddd1bf, 0x2b251d, 0x7a6e5e, 0xaa9e8d, 0xd9731a),
    ),
    (
        "mint",
        light(0xffffff, 0xcfe0d6, 0x14261c, 0x54705f, 0x93ab9d, 0x1f9d5c),
    ),
];

pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let h = text.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(hex)
}

pub fn look(theme: &str, accent: Option<&str>, system_dark: bool) -> Look {
    let mut l = THEMES
        .iter()
        .find(|(id, _)| *id == theme)
        .map(|(_, l)| *l)
        .unwrap_or(if system_dark {
            SYSTEM_DARK
        } else {
            SYSTEM_LIGHT
        });
    if let Some(a) = accent.and_then(parse_hex) {
        l.accent = a;
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_theme_follows_the_system() {
        assert_eq!(look("system", None, true), SYSTEM_DARK);
        assert_eq!(look("system", None, false), SYSTEM_LIGHT);
        assert_eq!(look("nope", None, true), SYSTEM_DARK);
    }

    #[test]
    fn a_fixed_theme_ignores_the_system() {
        assert!(look("midnight", None, false).dark);
        assert!(!look("sand", None, true).dark);
    }

    #[test]
    fn an_accent_is_laid_over_any_theme() {
        let l = look("ember", Some("#0a84ff"), true);
        assert_eq!(l.accent, [0x0a, 0x84, 0xff]);
        assert_eq!(l.card, look("ember", None, true).card);
        assert_eq!(
            look("ember", Some("garbage"), true).accent,
            [0xf2, 0x6a, 0x1f]
        );
    }
}
