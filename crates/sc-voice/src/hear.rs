use sc_core::store::{self, Manifest};
use serde_json::{Map, Value};
use std::time::{Duration, Instant};

pub const PHRASE: &str = "simple clip that";
pub const COOLDOWN: Duration = Duration::from_secs(5);
const SHOW_SAVE_FOR: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    pub words: Vec<String>,
}

impl Phrase {
    pub fn new(text: &str) -> Self {
        let words: Vec<String> = normalize(text)
            .split_whitespace()
            .map(String::from)
            .collect();
        if words.len() < 2 {
            return Self::default();
        }
        Self { words }
    }

    pub fn text(&self) -> String {
        self.words.join(" ")
    }

    pub fn head(&self) -> String {
        self.words[..self.words.len() - 1].join(" ")
    }

    pub fn tail(&self) -> &str {
        self.words.last().map(String::as_str).unwrap_or_default()
    }

    pub fn heads(&self) -> Vec<String> {
        let head = self.head();
        let joined = head.replace("simple clip", "simpleclip");
        if joined == head {
            vec![head]
        } else {
            vec![head, joined]
        }
    }

    fn key_word(&self) -> &str {
        if let Some(w) = self.words.iter().find(|w| w.as_str() == "clip") {
            return w;
        }
        self.words
            .iter()
            .max_by_key(|w| w.chars().count())
            .map(String::as_str)
            .unwrap_or_default()
    }
}

impl Default for Phrase {
    fn default() -> Self {
        Self {
            words: PHRASE.split(' ').map(String::from).collect(),
        }
    }
}

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

fn key_confidence(h: &Heard, key: &str) -> Option<f32> {
    h.text
        .split_whitespace()
        .zip(&h.words)
        .find(|(w, _)| normalize(w).replace(' ', "").contains(key))
        .map(|(_, c)| *c)
}

pub fn normalize(text: &str) -> String {
    let kept: String = text
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    kept.split_whitespace()
        .map(|w| if w == "simpleclip" { "simple clip" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

pub const OPENERS: [&str; 4] = ["hey", "okay", "ok", "yo"];

pub fn is_phrase(text: &str, p: &Phrase) -> bool {
    let n = normalize(text);
    let want = p.text();
    n == want
        || OPENERS
            .iter()
            .find_map(|o| n.strip_prefix(&format!("{o} ")))
            .is_some_and(|rest| rest == want)
}

pub fn accepts(h: &Heard, p: &Phrase, s: Strictness) -> bool {
    let (whole, word) = s.floor();
    let words = match s {
        Strictness::Normal => key_confidence(h, p.key_word()).is_some_and(|c| c >= word),
        Strictness::Strict => h.words.iter().all(|&w| w >= word),
    };
    h.command && is_phrase(&h.text, p) && h.confidence >= whole && words
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
    pub card: bool,
    pub strictness: Strictness,
    pub phrase: Phrase,
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
        let card = store::value(m, values, "card")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let strictness = store::value(m, values, "sensitivity")
            .and_then(|v| v.as_str().map(Strictness::from_name))
            .unwrap_or(Strictness::Normal);
        let phrase = store::value(m, values, "phrase")
            .and_then(|v| v.as_str().map(Phrase::new))
            .unwrap_or_default();
        Self {
            enabled: store::is_enabled(values),
            secs,
            beep,
            card,
            strictness,
            phrase,
        }
    }
}

pub const SHOW_FOR: Duration = Duration::from_secs(2);

pub fn save_delay(s: &Settings) -> Duration {
    if s.card || s.beep {
        SHOW_FOR
    } else {
        Duration::ZERO
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

pub fn status_line(
    enabled: bool,
    phrase: &Phrase,
    mic: Ear,
    last: Option<&Last>,
    now: Instant,
) -> String {
    let said = phrase.text();
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
                Err(e) => format!("Heard \"{said}\" {} but could not save: {e}", ago(since)),
            };
        }
    }
    format!("Listening for \"{said}\"")
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
            "hey simple clip that",
            "Hey, SimpleClip that",
            "okay simple clip that",
            "ok simpleclip that",
            "yo simple clip that",
        ] {
            assert!(is_phrase(t, &Phrase::default()), "{t}");
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
            "hey hey simple clip that",
            "hey simple clip",
            "hey that",
        ] {
            assert!(!is_phrase(t, &Phrase::default()), "{t}");
        }
    }

    #[test]
    fn confidence_must_reach_the_chosen_level() {
        let p = Phrase::default();
        assert!(accepts(
            &heard("simple clip that", 0.22),
            &p,
            Strictness::Normal
        ));
        assert!(!accepts(
            &heard("simple clip that", 0.4),
            &p,
            Strictness::Strict
        ));
        assert!(accepts(
            &heard("simple clip that", 0.6),
            &p,
            Strictness::Strict
        ));
        assert!(!accepts(
            &heard("simple clip that", 0.1),
            &p,
            Strictness::Normal
        ));
        assert!(!accepts(
            &heard("simple clip is great", 0.99),
            &p,
            Strictness::Normal
        ));
    }

    #[test]
    fn a_phrase_of_your_own_is_tidied_and_needs_two_words() {
        let p = Phrase::new("  Save, THAT  Play! ");
        assert_eq!(p.text(), "save that play");
        assert_eq!((p.head(), p.tail()), ("save that".to_string(), "play"));
        assert_eq!(p.heads(), ["save that"]);
        assert_ne!(p, Phrase::default());
        assert_eq!(Phrase::new("clip"), Phrase::default());
        assert_eq!(Phrase::new("  "), Phrase::default());
        assert_eq!(Phrase::new("SimpleClip that"), Phrase::default());
        assert_eq!(Phrase::default().heads(), ["simple clip", "simpleclip"]);
        assert_eq!(Phrase::new("let's go simple clip it").heads().len(), 2);
    }

    #[test]
    fn your_own_phrase_triggers_and_the_old_one_does_not() {
        let p = Phrase::new("clip it now");
        for t in [
            "clip it now",
            "Clip it, now!",
            "hey clip it now",
            "okay clip it now",
        ] {
            assert!(is_phrase(t, &p), "{t}");
        }
        for t in [
            "simple clip that",
            "clip it",
            "clip it now please",
            "hey clip",
        ] {
            assert!(!is_phrase(t, &p), "{t}");
        }
        assert!(accepts(&heard("clip it now", 0.3), &p, Strictness::Normal));
        assert!(!accepts(
            &heard("simple clip that", 0.9),
            &p,
            Strictness::Normal
        ));
        let hey = Phrase::new("hey buddy save it");
        assert!(is_phrase("hey buddy save it", &hey));
        assert!(is_phrase("okay hey buddy save it", &hey));
        let mut h = heard("nice shot buddy", 0.4);
        let nice = Phrase::new("nice shot buddy");
        assert!(accepts(&h, &nice, Strictness::Normal));
        h.words = vec![0.9, 0.9, 0.0];
        assert!(
            !accepts(&h, &nice, Strictness::Normal),
            "the longest word must be heard"
        );
    }

    #[test]
    fn the_phrase_setting_is_read() {
        let m = manifest();
        let v: Map<String, Value> = serde_json::from_str(r#"{"phrase":"Clip It Now"}"#).unwrap();
        assert_eq!(Settings::read(&m, &v).phrase.text(), "clip it now");
        let v: Map<String, Value> = serde_json::from_str(r#"{"phrase":"go"}"#).unwrap();
        assert_eq!(Settings::read(&m, &v).phrase, Phrase::default());
        let now = Instant::now();
        assert_eq!(
            status_line(true, &Phrase::new("clip it now"), Ear::Listening, None, now),
            "Listening for \"clip it now\""
        );
    }

    #[test]
    fn one_badly_heard_word_is_enough_to_refuse() {
        let mut h = heard("simple clip that", 0.8);
        h.words = vec![0.9, 0.0, 0.9];
        assert!(!accepts(&h, &Phrase::default(), Strictness::Normal));
        h.words = vec![0.0, 0.02, 0.61];
        h.confidence = 0.21;
        assert!(accepts(&h, &Phrase::default(), Strictness::Normal));
        assert!(!accepts(&h, &Phrase::default(), Strictness::Strict));
        h.words = vec![0.9, 0.3, 0.9];
        h.confidence = 0.8;
        assert!(accepts(&h, &Phrase::default(), Strictness::Strict));
        let mut joined = heard("simpleclip that", 0.4);
        joined.words = vec![0.27, 0.62];
        assert!(accepts(&joined, &Phrase::default(), Strictness::Normal));
        joined.words = vec![0.0, 0.62];
        assert!(!accepts(&joined, &Phrase::default(), Strictness::Normal));
    }

    #[test]
    fn only_the_command_grammar_can_trigger() {
        let mut h = heard("simple clip that", 0.99);
        h.command = false;
        assert!(!accepts(&h, &Phrase::default(), Strictness::Normal));
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
        assert_eq!(m.settings.len(), 5);
        let s = Settings::read(&m, &Map::new());
        assert_eq!(
            s,
            Settings {
                enabled: true,
                secs: None,
                beep: true,
                card: true,
                strictness: Strictness::Normal,
                phrase: Phrase::default(),
            }
        );
    }

    #[test]
    fn settings_follow_the_file_and_stay_in_range() {
        let m = manifest();
        let v: Map<String, Value> = serde_json::from_str(
            r#"{"enabled":false,"secs":900,"beep":false,"card":false,"sensitivity":"Strict"}"#,
        )
        .unwrap();
        let s = Settings::read(&m, &v);
        assert_eq!(
            s,
            Settings {
                enabled: false,
                secs: Some(300),
                beep: false,
                card: false,
                strictness: Strictness::Strict,
                phrase: Phrase::default(),
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
        assert_eq!(
            status_line(false, &Phrase::default(), Ear::Listening, None, now),
            "Off"
        );
        assert_eq!(
            status_line(true, &Phrase::default(), Ear::NoMic, None, now),
            "No microphone found"
        );
        assert_eq!(
            status_line(true, &Phrase::default(), Ear::NoSpeech, None, now),
            "Windows speech recognition is not available"
        );
        assert_eq!(
            status_line(true, &Phrase::default(), Ear::Listening, None, now),
            listening
        );
        let ok = Last {
            at: now,
            result: Ok(()),
        };
        assert_eq!(
            status_line(
                true,
                &Phrase::default(),
                Ear::Listening,
                Some(&ok),
                now + Duration::from_secs(12)
            ),
            "Saved a clip 12 s ago"
        );
        assert_eq!(
            status_line(
                true,
                &Phrase::default(),
                Ear::Listening,
                Some(&ok),
                now + Duration::from_secs(200)
            ),
            "Saved a clip 3 min ago"
        );
        assert_eq!(
            status_line(
                true,
                &Phrase::default(),
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
                &Phrase::default(),
                Ear::Listening,
                Some(&bad),
                now + Duration::from_secs(3)
            ),
            "Heard \"simple clip that\" 3 s ago but could not save: SimpleClip is not running"
        );
    }
}
