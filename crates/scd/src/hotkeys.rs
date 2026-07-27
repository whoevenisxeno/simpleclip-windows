use evdev::{InputEventKind, Key};
use std::sync::{Arc, Mutex};

/// In-app global hotkey listener. SimpleClip reads keyboard events directly
/// (evdev), so the save hotkey is configured entirely in the app rather than in
/// the compositor. Requires read access to /dev/input (the `input` group).
#[derive(Default, Clone, Copy, PartialEq)]
struct Mods {
    sup: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

struct Binding {
    mods: Mods,
    key: u16,
}

fn parse(spec: &str) -> Option<Binding> {
    let parts: Vec<&str> = spec
        .split('+')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let (key_tok, mod_toks) = parts.split_last()?;
    let mut mods = Mods::default();
    for m in mod_toks {
        match m.to_uppercase().as_str() {
            "SUPER" | "META" | "WIN" | "MOD" => mods.sup = true,
            "CTRL" | "CONTROL" => mods.ctrl = true,
            "ALT" => mods.alt = true,
            "SHIFT" => mods.shift = true,
            _ => return None,
        }
    }
    Some(Binding {
        mods,
        key: key_code(&key_tok.to_uppercase())?,
    })
}

/// Map a key name to its Linux input-event code.
fn key_code(k: &str) -> Option<u16> {
    if let Some(n) = k.strip_prefix('F').and_then(|d| d.parse::<u8>().ok()) {
        return match n {
            1..=10 => Some(58 + n as u16),
            11 => Some(87),
            12 => Some(88),
            _ => None,
        };
    }
    if k.chars().count() == 1 {
        let c = k.chars().next().unwrap();
        if c.is_ascii_digit() {
            return Some(if c == '0' {
                11
            } else {
                2 + (c as u16 - '1' as u16)
            });
        }
        const LETTERS: &[(char, u16)] = &[
            ('A', 30),
            ('B', 48),
            ('C', 46),
            ('D', 32),
            ('E', 18),
            ('F', 33),
            ('G', 34),
            ('H', 35),
            ('I', 23),
            ('J', 36),
            ('K', 37),
            ('L', 38),
            ('M', 50),
            ('N', 49),
            ('O', 24),
            ('P', 25),
            ('Q', 16),
            ('R', 19),
            ('S', 31),
            ('T', 20),
            ('U', 22),
            ('V', 47),
            ('W', 17),
            ('X', 45),
            ('Y', 21),
            ('Z', 44),
        ];
        return LETTERS
            .iter()
            .find(|(ch, _)| *ch == c)
            .map(|(_, code)| *code);
    }
    match k {
        "SPACE" => Some(57),
        "ENTER" | "RETURN" => Some(28),
        "TAB" => Some(15),
        "ESC" | "ESCAPE" => Some(1),
        _ => None,
    }
}

fn mod_of(code: u16) -> Option<u8> {
    match code {
        29 | 97 => Some(0),   // ctrl
        42 | 54 => Some(1),   // shift
        56 | 100 => Some(2),  // alt
        125 | 126 => Some(3), // super
        _ => None,
    }
}

/// Start listening. Reads every keyboard but only ever acts on the configured
/// combo (it does not log or store keystrokes). Spawns one thread per keyboard.
pub fn spawn(spec: &str, on_trigger: Arc<dyn Fn() + Send + Sync>) {
    let Some(binding) = parse(spec) else {
        tracing::warn!(
            hotkey = spec,
            "unparseable save hotkey; in-app hotkey disabled"
        );
        return;
    };
    let devices: Vec<_> = evdev::enumerate()
        .filter(|(_, d)| {
            d.supported_keys()
                .is_some_and(|k| k.contains(Key::KEY_ENTER))
        })
        .collect();
    if devices.is_empty() {
        tracing::warn!(
            "no readable keyboards; add your user to the 'input' group for in-app hotkeys"
        );
        return;
    }
    tracing::info!(
        hotkey = spec,
        keyboards = devices.len(),
        "in-app hotkey listener active"
    );

    let mods = Arc::new(Mutex::new(Mods::default()));
    let (key, want) = (binding.key, binding.mods);
    for (_, mut dev) in devices {
        let mods = mods.clone();
        let cb = on_trigger.clone();
        std::thread::spawn(move || loop {
            let Ok(events) = dev.fetch_events() else {
                break;
            };
            for ev in events {
                let InputEventKind::Key(k) = ev.kind() else {
                    continue;
                };
                let code = k.code();
                if let Some(idx) = mod_of(code) {
                    let down = ev.value() != 0;
                    let mut m = mods.lock().unwrap();
                    match idx {
                        0 => m.ctrl = down,
                        1 => m.shift = down,
                        2 => m.alt = down,
                        _ => m.sup = down,
                    }
                } else if code == key && ev.value() == 1 && *mods.lock().unwrap() == want {
                    cb();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_super_f10() {
        let b = parse("SUPER+F10").unwrap();
        assert_eq!(b.key, 68);
        assert!(b.mods.sup && !b.mods.ctrl);
    }

    #[test]
    fn parses_multi_mod() {
        let b = parse("CTRL+ALT+C").unwrap();
        assert_eq!(b.key, 46);
        assert!(b.mods.ctrl && b.mods.alt);
    }

    #[test]
    fn rejects_bad_modifier() {
        assert!(parse("FOO+X").is_none());
    }
}
