use sc_core::store::{self, Manifest};
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

pub const PHRASE: &str = "simple clip that";
pub const COOLDOWN: Duration = Duration::from_secs(5);
const SHOW_SAVE_FOR: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub text: String,
    pub confidence: f32,
    pub words: Vec<f32>,
    pub command: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    Normal,
    Strict,
}

impl Strictness {
    pub fn from_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("strict") {
            Self::Strict
        } else {
            Self::Normal
        }
    }

    pub fn floor(self) -> (f32, f32) {
        match self {
            Self::Normal => (0.15, 0.01),
            Self::Strict => (0.5, 0.2),
        }
    }
}

fn clip_word(h: &Heard) -> Option<f32> {
    h.text
        .split_whitespace()
        .zip(&h.words)
        .find(|(w, _)| w.to_lowercase().contains("clip"))
        .map(|(_, c)| *c)
}

pub fn normalize(text: &str) -> String {
    let kept: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    kept.split_whitespace()
        .map(|w| if w == "simpleclip" { "simple clip" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_phrase(text: &str) -> bool {
    normalize(text) == PHRASE
}

pub fn accepts(h: &Heard, s: Strictness) -> bool {
    let (whole, word) = s.floor();
    let words = match s {
        Strictness::Normal => clip_word(h).is_some_and(|c| c >= word),
        Strictness::Strict => h.words.iter().all(|&w| w >= word),
    };
    h.command && is_phrase(&h.text) && h.confidence >= whole && words
}

pub struct Cooldown {
    gap: Duration,
    last: Option<Instant>,
}

impl Cooldown {
    pub fn new(gap: Duration) -> Self {
        Self { gap, last: None }
    }

    pub fn take(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|t| now.saturating_duration_since(t) < self.gap)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub secs: Option<u32>,
    pub beep: bool,
    pub strictness: Strictness,
}

pub fn manifest() -> Manifest {
    serde_json::from_str(include_str!("../manifest.json")).unwrap_or_default()
}

impl Settings {
    pub fn read(m: &Manifest, values: &Map<String, Value>) -> Self {
        let secs = store::value(m, values, "secs")
            .and_then(|v| v.as_u64())
            .filter(|&s| s > 0)
            .map(|s| s.min(u32::MAX as u64) as u32);
        let beep = store::value(m, values, "beep")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let strictness = store::value(m, values, "sensitivity")
            .and_then(|v| v.as_str().map(Strictness::from_name))
            .unwrap_or(Strictness::Normal);
        Self {
            enabled: store::is_enabled(values),
            secs,
            beep,
            strictness,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ear {
    Starting,
    Listening,
    NoMic,
    NoSpeech,
}

pub struct Last {
    pub at: Instant,
    pub result: Result<(), String>,
}

fn ago(d: Duration) -> String {
    let s = d.as_secs();
    if s < 120 {
        format!("{s} s ago")
    } else {
        format!("{} min ago", s / 60)
    }
}

pub fn status_line(enabled: bool, mic: Ear, last: Option<&Last>, now: Instant) -> String {
    if !enabled {
        return "Off".into();
    }
    match mic {
        Ear::NoSpeech => return "Windows speech recognition is not available".into(),
        Ear::NoMic => return "No microphone found".into(),
        Ear::Starting => return "Starting the microphone".into(),
        Ear::Listening => {}
    }
    if let Some(l) = last {
        let since = now.saturating_duration_since(l.at);
        if since < SHOW_SAVE_FOR {
            return match &l.result {
                Ok(()) => format!("Saved a clip {}", ago(since)),
                Err(e) => format!("Heard \"{PHRASE}\" {} but could not save: {e}", ago(since)),
            };
        }
    }
    format!("Listening for \"{PHRASE}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(text: &str, confidence: f32) -> Heard {
        Heard {
            text: text.into(),
            confidence,
            words: vec![confidence; 3],
            command: true,
        }
    }

    #[test]
    fn both_spellings_of_the_phrase_match() {
        for t in [
            "simple clip that",
            "Simple Clip that.",
            "simpleclip that",
            "  SimpleClip,  that! ",
        ] {
            assert!(is_phrase(t), "{t}");
        }
    }

    #[test]
    fn other_sentences_do_not_match() {
        for t in [
            "",
            "clip that",
            "simple clip",
            "simple clip is great",
            "simple clip that was fun",
            "simple clips that",
            "i said simple clip that",
        ] {
            assert!(!is_phrase(t), "{t}");
        }
    }

    #[test]
    fn confidence_must_reach_the_chosen_level() {
        assert!(accepts(
            &heard("simple clip that", 0.22),
            Strictness::Normal
        ));
        assert!(!accepts(
            &heard("simple clip that", 0.4),
            Strictness::Strict
        ));
        assert!(accepts(&heard("simple clip that", 0.6), Strictness::Strict));
        assert!(!accepts(
            &heard("simple clip that", 0.1),
            Strictness::Normal
        ));
        assert!(!accepts(
            &heard("simple clip is great", 0.99),
            Strictness::Normal
        ));
    }

    #[test]
    fn one_badly_heard_word_is_enough_to_refuse() {
        let mut h = heard("simple clip that", 0.8);
        h.words = vec![0.9, 0.0, 0.9];
        assert!(!accepts(&h, Strictness::Normal));
        h.words = vec![0.0, 0.02, 0.61];
        h.confidence = 0.21;
        assert!(accepts(&h, Strictness::Normal));
        assert!(!accepts(&h, Strictness::Strict));
        h.words = vec![0.9, 0.3, 0.9];
        h.confidence = 0.8;
        assert!(accepts(&h, Strictness::Strict));
        let mut joined = heard("simpleclip that", 0.4);
        joined.words = vec![0.27, 0.62];
        assert!(accepts(&joined, Strictness::Normal));
        joined.words = vec![0.0, 0.62];
        assert!(!accepts(&joined, Strictness::Normal));
    }

    #[test]
    fn only_the_command_grammar_can_trigger() {
        let mut h = heard("simple clip that", 0.99);
        h.command = false;
        assert!(!accepts(&h, Strictness::Normal));
    }

    #[test]
    fn the_cooldown_blocks_quick_repeats() {
        let t0 = Instant::now();
        let mut c = Cooldown::new(COOLDOWN);
        assert!(c.take(t0));
        assert!(!c.take(t0));
        assert!(!c.take(t0 + Duration::from_millis(4900)));
        assert!(c.take(t0 + Duration::from_secs(5)));
        assert!(!c.take(t0 + Duration::from_secs(6)));
        assert!(c.take(t0 + Duration::from_secs(11)));
    }

    #[test]
    fn settings_use_the_manifest_defaults() {
        let m = manifest();
        assert_eq!(m.name, "voice");
        assert_eq!(m.exe_name(), "sc-voice.exe");
        assert_eq!(m.settings.len(), 3);
        let s = Settings::read(&m, &Map::new());
        assert_eq!(
            s,
            Settings {
                enabled: true,
                secs: None,
                beep: true,
                strictness: Strictness::Normal
            }
        );
    }

    #[test]
    fn settings_follow_the_file_and_stay_in_range() {
        let m = manifest();
        let v: Map<String, Value> = serde_json::from_str(
            r#"{"enabled":false,"secs":900,"beep":false,"sensitivity":"Strict"}"#,
        )
        .unwrap();
        let s = Settings::read(&m, &v);
        assert_eq!(
            s,
            Settings {
                enabled: false,
                secs: Some(300),
                beep: false,
                strictness: Strictness::Strict
            }
        );
        let v: Map<String, Value> =
            serde_json::from_str(r#"{"secs":20,"sensitivity":"Loud"}"#).unwrap();
        let s = Settings::read(&m, &v);
        assert_eq!((s.secs, s.strictness), (Some(20), Strictness::Normal));
        let v: Map<String, Value> = serde_json::from_str(r#"{"secs":-5}"#).unwrap();
        assert_eq!(Settings::read(&m, &v).secs, None);
    }

    #[test]
    fn the_status_line_explains_what_is_going_on() {
        let now = Instant::now();
        let listening = format!("Listening for \"{PHRASE}\"");
        assert_eq!(status_line(false, Ear::Listening, None, now), "Off");
        assert_eq!(
            status_line(true, Ear::NoMic, None, now),
            "No microphone found"
        );
        assert_eq!(
            status_line(true, Ear::NoSpeech, None, now),
            "Windows speech recognition is not available"
        );
        assert_eq!(status_line(true, Ear::Listening, None, now), listening);
        let ok = Last {
            at: now,
            result: Ok(()),
        };
        assert_eq!(
            status_line(
                true,
                Ear::Listening,
                Some(&ok),
                now + Duration::from_secs(12)
            ),
            "Saved a clip 12 s ago"
        );
        assert_eq!(
            status_line(
                true,
                Ear::Listening,
                Some(&ok),
                now + Duration::from_secs(200)
            ),
            "Saved a clip 3 min ago"
        );
        assert_eq!(
            status_line(
                true,
                Ear::Listening,
                Some(&ok),
                now + Duration::from_secs(900)
            ),
            listening
        );
        let bad = Last {
            at: now,
            result: Err("SimpleClip is not running".into()),
        };
        assert_eq!(
            status_line(
                true,
                Ear::Listening,
                Some(&bad),
                now + Duration::from_secs(3)
            ),
            "Heard \"simple clip that\" 3 s ago but could not save: SimpleClip is not running"
        );
    }
}
