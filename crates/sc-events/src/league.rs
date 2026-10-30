use serde::Deserialize;
use std::process::Command;

const BASE: &str = "https://127.0.0.1:2999/liveclientdata";

#[derive(Debug, Clone, PartialEq)]
pub struct LolEvent {
    pub id: u64,
    pub name: String,
    pub killer: Option<String>,
    pub streak: Option<u32>,
}

#[derive(Deserialize)]
struct RawEvents {
    #[serde(rename = "Events", default)]
    events: Vec<RawEvent>,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(rename = "EventID", default)]
    id: u64,
    #[serde(rename = "EventName", default)]
    name: String,
    #[serde(rename = "KillerName")]
    killer: Option<String>,
    #[serde(rename = "KillStreak")]
    streak: Option<u32>,
}

pub fn parse_events(json: &str) -> Vec<LolEvent> {
    serde_json::from_str::<RawEvents>(json)
        .map(|r| {
            r.events
                .into_iter()
                .map(|e| LolEvent {
                    id: e.id,
                    name: e.name,
                    killer: e.killer,
                    streak: e.streak,
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn normalize(name: &str) -> String {
    let name = name.trim().trim_matches('"');
    name.split('#').next().unwrap_or(name).trim().to_lowercase()
}

pub fn parse_names(active_name: &str, active_player: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut add = |n: &str| {
        let n = normalize(n);
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    };
    add(&serde_json::from_str::<String>(active_name).unwrap_or_else(|_| active_name.to_string()));
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(active_player) {
        for key in ["riotIdGameName", "summonerName", "riotId"] {
            if let Some(n) = v[key].as_str() {
                add(n);
            }
        }
    }
    names
}

#[derive(Default)]
pub struct Tracker {
    seen: Option<u64>,
}

impl Tracker {
    pub fn reset(&mut self) {
        self.seen = None;
    }

    pub fn feed(&mut self, me: &[String], events: &[LolEvent]) -> Option<u32> {
        let newest = events.iter().map(|e| e.id).max().unwrap_or(0);
        let Some(last) = self.seen else {
            self.seen = Some(newest);
            return None;
        };
        self.seen = Some(newest.max(last));
        let mut best = None;
        for e in events.iter().filter(|e| e.id > last) {
            let mine = e
                .killer
                .as_deref()
                .is_some_and(|k| me.contains(&normalize(k)));
            if !mine {
                continue;
            }
            let kills = match e.name.as_str() {
                "ChampionKill" => 1,
                "Multikill" => e.streak.unwrap_or(2),
                _ => continue,
            };
            best = best.max(Some(kills));
        }
        best
    }
}

fn get(path: &str) -> Option<String> {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-sk", "--max-time", "2", &format!("{BASE}/{path}")]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        .filter(|t| !t.trim().is_empty())
}

pub struct Snapshot {
    pub me: Vec<String>,
    pub events: Vec<LolEvent>,
}

pub fn poll() -> Option<Snapshot> {
    let events = parse_events(&get("eventdata")?);
    let name = get("activeplayername").unwrap_or_default();
    let player = get("activeplayer").unwrap_or_default();
    let me = parse_names(&name, &player);
    (!me.is_empty()).then_some(Snapshot { me, events })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"Events":[
        {"EventID":0,"EventName":"GameStart","EventTime":0.05},
        {"EventID":1,"EventName":"MinionsSpawning","EventTime":65.0},
        {"EventID":2,"EventName":"ChampionKill","EventTime":312.4,"KillerName":"Zed Main","VictimName":"Ahri"},
        {"EventID":3,"EventName":"Multikill","EventTime":318.0,"KillerName":"Zed Main","KillStreak":2}]}"#;

    fn me() -> Vec<String> {
        vec!["zed main".to_string()]
    }

    fn ev(id: u64, name: &str, killer: Option<&str>, streak: Option<u32>) -> LolEvent {
        LolEvent {
            id,
            name: name.into(),
            killer: killer.map(str::to_string),
            streak,
        }
    }

    #[test]
    fn events_are_read_from_the_live_client_reply() {
        let events = parse_events(SAMPLE);
        assert_eq!(events.len(), 4);
        assert_eq!(events[2].name, "ChampionKill");
        assert_eq!(events[2].killer.as_deref(), Some("Zed Main"));
        assert_eq!(events[3].streak, Some(2));
        assert!(parse_events("not json").is_empty());
        assert!(parse_events("{}").is_empty());
    }

    #[test]
    fn names_ignore_case_quotes_and_the_tag() {
        assert_eq!(normalize("\"Zed Main#EUW1\""), "zed main");
        assert_eq!(normalize("  Zed Main "), "zed main");
        let names = parse_names(
            "\"Zed Main#EUW\"",
            r#"{"riotIdGameName":"Zed Main","summonerName":"OldName"}"#,
        );
        assert_eq!(names, vec!["zed main".to_string(), "oldname".to_string()]);
    }

    #[test]
    fn history_from_before_we_connected_is_not_a_trigger() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(&me(), &parse_events(SAMPLE)), None);
        assert_eq!(t.feed(&me(), &parse_events(SAMPLE)), None);
    }

    #[test]
    fn a_new_multikill_by_me_reports_its_streak() {
        let mut t = Tracker::default();
        let mut events = parse_events(SAMPLE);
        t.feed(&me(), &events);
        events.push(ev(4, "ChampionKill", Some("Zed Main"), None));
        events.push(ev(5, "Multikill", Some("Zed Main"), Some(3)));
        assert_eq!(t.feed(&me(), &events), Some(3));
        assert_eq!(t.feed(&me(), &events), None);
    }

    #[test]
    fn a_single_kill_reports_one() {
        let mut t = Tracker::default();
        let mut events = vec![ev(0, "GameStart", None, None)];
        t.feed(&me(), &events);
        events.push(ev(1, "ChampionKill", Some("zed main#euw"), None));
        assert_eq!(t.feed(&me(), &events), Some(1));
    }

    #[test]
    fn other_players_kills_and_other_events_are_ignored() {
        let mut t = Tracker::default();
        let mut events = vec![ev(0, "GameStart", None, None)];
        t.feed(&me(), &events);
        events.push(ev(1, "ChampionKill", Some("Ahri"), None));
        events.push(ev(2, "Multikill", Some("Ahri"), Some(5)));
        events.push(ev(3, "TurretKilled", Some("Zed Main"), None));
        events.push(ev(4, "Ace", Some("Zed Main"), None));
        assert_eq!(t.feed(&me(), &events), None);
    }

    #[test]
    fn a_reset_forgets_the_old_game() {
        let mut t = Tracker::default();
        t.feed(&me(), &parse_events(SAMPLE));
        t.reset();
        let fresh = vec![
            ev(0, "GameStart", None, None),
            ev(1, "ChampionKill", Some("Zed Main"), None),
        ];
        assert_eq!(t.feed(&me(), &fresh), None);
    }
}
