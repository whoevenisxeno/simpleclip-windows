use crate::layout::Layout;
use crate::look::Theme;
use sc_core::store::{self, Manifest};
use serde_json::{Map, Value};

pub const MANIFEST: &str = include_str!("../manifest.json");

pub fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST).expect("the built-in manifest is valid")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub at: [f32; 2],
    pub layout: Layout,
    pub size: u32,
    pub opacity: u32,
    pub theme: Theme,
    pub mouse: bool,
    pub games: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub layout: Layout,
    pub size: u32,
    pub opacity: u32,
    pub theme: Theme,
    pub mouse: bool,
}

impl Settings {
    pub fn read(m: &Manifest, values: &Map<String, Value>) -> Self {
        let get = |k: &str| store::value(m, values, k).unwrap_or(Value::Null);
        let text = |k: &str| get(k).as_str().unwrap_or_default().to_string();
        let num = |k: &str| get(k).as_i64().unwrap_or(100).max(1) as u32;
        let on = |k: &str| get(k).as_bool().unwrap_or(true);
        Settings {
            enabled: store::is_enabled(values),
            at: store::place_of(&get("where")).unwrap_or([0.02, 0.72]),
            layout: Layout::from_name(&text("layout")),
            size: num("size"),
            opacity: num("opacity"),
            theme: Theme::from_name(&text("theme")),
            mouse: on("mouse"),
            games: on("games"),
        }
    }

    pub fn shape(&self) -> Shape {
        Shape {
            layout: self.layout,
            size: self.size,
            opacity: self.opacity,
            theme: self.theme,
            mouse: self.mouse,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_defaults_match_the_manifest() {
        let m = manifest();
        assert_eq!(m.name, "keys");
        assert_eq!(m.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(m.exe_name(), "sc-keys.exe");
        assert!(m.background);
        let s = Settings::read(&m, &Map::new());
        assert_eq!(
            s,
            Settings {
                enabled: true,
                at: [0.02, 0.72],
                layout: Layout::Wasd,
                size: 100,
                opacity: 90,
                theme: Theme::Dark,
                mouse: true,
                games: true,
            }
        );
    }

    #[test]
    fn saved_values_are_read_and_bad_ones_fall_back() {
        let m = manifest();
        let v = json!({"enabled": false, "where": [0.5, 2.0], "layout": "Full keyboard",
            "size": 999, "opacity": 5, "theme": "Neon", "mouse": false, "games": false});
        let s = Settings::read(&m, v.as_object().unwrap());
        assert!(!s.enabled && !s.mouse && !s.games);
        assert_eq!(s.at, [0.5, 1.0]);
        assert_eq!((s.layout, s.theme), (Layout::Full, Theme::Neon));
        assert_eq!((s.size, s.opacity), (200, 20));
        let v = json!({"layout": "Numpad", "theme": 3, "where": "left"});
        let s = Settings::read(&m, v.as_object().unwrap());
        assert_eq!(
            (s.layout, s.theme, s.at),
            (Layout::Wasd, Theme::Dark, [0.02, 0.72])
        );
    }
}
