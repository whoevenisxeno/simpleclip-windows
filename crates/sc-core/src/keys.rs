const NAMED: &[(&str, u32)] = &[
    ("BACKSPACE", 0x08),
    ("TAB", 0x09),
    ("ENTER", 0x0D),
    ("PAUSE", 0x13),
    ("CAPSLOCK", 0x14),
    ("ESCAPE", 0x1B),
    ("SPACE", 0x20),
    ("PAGEUP", 0x21),
    ("PAGEDOWN", 0x22),
    ("END", 0x23),
    ("HOME", 0x24),
    ("LEFT", 0x25),
    ("UP", 0x26),
    ("RIGHT", 0x27),
    ("DOWN", 0x28),
    ("PRINT", 0x2C),
    ("INSERT", 0x2D),
    ("DELETE", 0x2E),
    ("APPS", 0x5D),
    ("NUM0", 0x60),
    ("NUM1", 0x61),
    ("NUM2", 0x62),
    ("NUM3", 0x63),
    ("NUM4", 0x64),
    ("NUM5", 0x65),
    ("NUM6", 0x66),
    ("NUM7", 0x67),
    ("NUM8", 0x68),
    ("NUM9", 0x69),
    ("NUMMULTIPLY", 0x6A),
    ("NUMADD", 0x6B),
    ("NUMSUBTRACT", 0x6D),
    ("NUMDECIMAL", 0x6E),
    ("NUMDIVIDE", 0x6F),
    ("NUMLOCK", 0x90),
    ("SCROLLLOCK", 0x91),
    ("VOLUMEMUTE", 0xAD),
    ("VOLUMEDOWN", 0xAE),
    ("VOLUMEUP", 0xAF),
    ("MEDIANEXT", 0xB0),
    ("MEDIAPREV", 0xB1),
    ("MEDIASTOP", 0xB2),
    ("MEDIAPLAY", 0xB3),
    (";", 0xBA),
    ("=", 0xBB),
    (",", 0xBC),
    ("-", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("`", 0xC0),
    ("[", 0xDB),
    ("\\", 0xDC),
    ("]", 0xDD),
    ("'", 0xDE),
    ("OEM102", 0xE2),
];

const ALIASES: &[(&str, u32)] = &[
    ("PRINTSCREEN", 0x2C),
    ("PRTSC", 0x2C),
    ("INS", 0x2D),
    ("DEL", 0x2E),
    ("PGUP", 0x21),
    ("PGDN", 0x22),
    ("RETURN", 0x0D),
    ("ESC", 0x1B),
    ("BACKTICK", 0xC0),
    ("GRAVE", 0xC0),
    ("TILDE", 0xC0),
    ("PLUS", 0xBB),
    ("MINUS", 0xBD),
    ("COMMA", 0xBC),
    ("PERIOD", 0xBE),
    ("SLASH", 0xBF),
    ("SEMICOLON", 0xBA),
    ("QUOTE", 0xDE),
    ("BACKSLASH", 0xDC),
];

fn is_reserved(vk: u32) -> bool {
    matches!(vk, 0x00..=0x07 | 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5 | 0xE5 | 0xE7 | 0xFF)
}

pub fn vk_from_name(name: &str) -> Option<u32> {
    let k = name.trim().to_ascii_uppercase();
    if let Some(n) = k.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&n).then_some(0x6F + n);
    }
    if k.len() == 1 {
        let c = k.as_bytes()[0];
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(c as u32);
        }
    }
    if let Some(hex) = k.strip_prefix("VK_") {
        let vk = u32::from_str_radix(hex, 16).ok()?;
        return (vk <= 0xFE && !is_reserved(vk)).then_some(vk);
    }
    NAMED
        .iter()
        .chain(ALIASES)
        .find(|(n, _)| *n == k)
        .map(|(_, vk)| *vk)
}

pub fn name_from_vk(vk: u32) -> Option<String> {
    if is_reserved(vk) {
        return None;
    }
    Some(match vk {
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        _ => match NAMED.iter().find(|(_, v)| *v == vk) {
            Some((n, _)) => (*n).to_string(),
            None => format!("VK_{vk:02X}"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_usable_key_round_trips() {
        for vk in 0..=0xFFu32 {
            match name_from_vk(vk) {
                Some(name) => assert_eq!(vk_from_name(&name), Some(vk), "{name} ({vk:#x})"),
                None => assert!(is_reserved(vk)),
            }
        }
    }

    #[test]
    fn punctuation_and_navigation_keys() {
        assert_eq!(vk_from_name("`"), Some(0xC0));
        assert_eq!(vk_from_name("backtick"), Some(0xC0));
        assert_eq!(vk_from_name("-"), Some(0xBD));
        assert_eq!(vk_from_name("left"), Some(0x25));
        assert_eq!(vk_from_name("NumAdd"), Some(0x6B));
        assert_eq!(vk_from_name("F24"), Some(0x87));
        assert_eq!(vk_from_name("7"), Some(b'7' as u32));
    }

    #[test]
    fn unnamed_keys_use_hex() {
        assert_eq!(name_from_vk(0xFA).as_deref(), Some("VK_FA"));
        assert_eq!(vk_from_name("vk_fa"), Some(0xFA));
    }

    #[test]
    fn rejects_modifiers_and_nonsense() {
        assert!(vk_from_name("VK_11").is_none());
        assert!(vk_from_name("VK_FF").is_none());
        assert!(vk_from_name("VK_ZZ").is_none());
        assert!(vk_from_name("F25").is_none());
        assert!(vk_from_name("NOPE").is_none());
        assert!(vk_from_name("").is_none());
    }
}
