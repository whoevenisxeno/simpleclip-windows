use serde::Deserialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const APP_ID: u64 = 570;
const STREAK_WINDOW: Duration = Duration::from_secs(18);

#[derive(Debug, Default, Deserialize)]
pub struct Payload {
    provider: Option<Provider>,
    map: Option<MapInfo>,
    player: Option<Player>,
}

#[derive(Debug, Default, Deserialize)]
struct Provider {
    appid: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct MapInfo {
    matchid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Player {
    kills: Option<u32>,
}

impl Payload {
    pub fn is_dota(&self) -> bool {
        self.provider.as_ref().and_then(|p| p.appid) == Some(APP_ID)
    }

    fn local(&self) -> Option<(Option<&str>, u32)> {
        if !self.is_dota() {
            return None;
        }
        let kills = self.player.as_ref()?.kills?;
        let id = self.map.as_ref().and_then(|m| m.matchid.as_deref());
        Some((id, kills))
    }
}

#[derive(Default)]
pub struct Tracker {
    seen: bool,
    match_id: Option<String>,
    kills: u32,
    recent: Vec<Instant>,
}

impl Tracker {
    pub fn feed(&mut self, p: &Payload, now: Instant) -> Option<u32> {
        let (id, kills) = p.local()?;
        let same_match = self.match_id.as_deref() == id;
        if !self.seen || !same_match || kills < self.kills {
            self.seen = true;
            self.match_id = id.map(str::to_string);
            self.kills = kills;
            self.recent.clear();
            return None;
        }
        let fresh = kills - self.kills;
        self.kills = kills;
        if fresh == 0 {
            return None;
        }
        for _ in 0..fresh {
            self.recent.push(now);
        }
        self.recent
            .retain(|t| now.duration_since(*t) <= STREAK_WINDOW);
        Some(self.recent.len() as u32)
    }
}

pub fn cfg_text(port: u16) -> String {
    format!(
        "\"SimpleClip\"\n{{\n    \"uri\" \"http://127.0.0.1:{port}/\"\n    \"timeout\" \"5.0\"\n    \"buffer\" \"0.1\"\n    \"throttle\" \"0.1\"\n    \"heartbeat\" \"10.0\"\n    \"data\"\n    {{\n        \"provider\" \"1\"\n        \"map\" \"1\"\n        \"player\" \"1\"\n    }}\n}}\n"
    )
}

pub fn cfg_dirs(libraries: &[PathBuf]) -> Vec<PathBuf> {
    libraries
        .iter()
        .map(|l| {
            l.join("steamapps")
                .join("common")
                .join("dota 2 beta")
                .join("game")
                .join("dota")
                .join("cfg")
        })
        .filter(|d| d.is_dir())
        .map(|d| d.join("gamestate_integration"))
        .collect()
}

pub fn locate() -> Vec<PathBuf> {
    crate::cs2::steam_root()
        .map(|root| cfg_dirs(&crate::cs2::steam_libraries(&root)))
        .unwrap_or_default()
}

pub fn install(dirs: &[PathBuf], port: u16) -> usize {
    let text = cfg_text(port);
    for dir in dirs {
        let _ = std::fs::create_dir_all(dir);
    }
    crate::cs2::install_text(dirs, &text)
}

pub fn remove(dirs: &[PathBuf]) -> usize {
    crate::cs2::remove(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(app: u64, match_id: &str, kills: u32) -> Payload {
        serde_json::from_str(&format!(
            r#"{{"provider":{{"name":"Dota 2","appid":{app},"version":47}},
                "map":{{"name":"start","matchid":"{match_id}","game_state":"DOTA_GAMERULES_STATE_GAME_IN_PROGRESS"}},
                "player":{{"steamid":"1","activity":"playing","kills":{kills},"deaths":0}}}}"#
        ))
        .unwrap()
    }

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn a_kill_is_reported_with_the_kills_made_just_before_it() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert_eq!(t.feed(&payload(570, "m1", 0), at(t0, 0)), None);
        assert_eq!(t.feed(&payload(570, "m1", 0), at(t0, 1)), None);
        assert_eq!(t.feed(&payload(570, "m1", 1), at(t0, 5)), Some(1));
        assert_eq!(t.feed(&payload(570, "m1", 1), at(t0, 6)), None);
        assert_eq!(t.feed(&payload(570, "m1", 2), at(t0, 12)), Some(2));
        assert_eq!(t.feed(&payload(570, "m1", 3), at(t0, 20)), Some(3));
    }

    #[test]
    fn kills_far_apart_do_not_build_a_streak() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.feed(&payload(570, "m1", 0), at(t0, 0));
        assert_eq!(t.feed(&payload(570, "m1", 1), at(t0, 10)), Some(1));
        assert_eq!(t.feed(&payload(570, "m1", 2), at(t0, 40)), Some(1));
    }

    #[test]
    fn two_kills_seen_in_one_update_count_as_two() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.feed(&payload(570, "m1", 4), at(t0, 0));
        assert_eq!(t.feed(&payload(570, "m1", 6), at(t0, 3)), Some(2));
    }

    #[test]
    fn joining_a_match_that_is_already_underway_does_not_trigger() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert_eq!(t.feed(&payload(570, "m1", 7), at(t0, 0)), None);
        assert_eq!(t.feed(&payload(570, "m1", 7), at(t0, 1)), None);
    }

    #[test]
    fn a_new_match_starts_from_scratch() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.feed(&payload(570, "m1", 0), at(t0, 0));
        t.feed(&payload(570, "m1", 9), at(t0, 5));
        assert_eq!(t.feed(&payload(570, "m2", 0), at(t0, 100)), None);
        assert_eq!(t.feed(&payload(570, "m2", 1), at(t0, 110)), Some(1));
    }

    #[test]
    fn other_games_and_menu_payloads_are_ignored() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert_eq!(t.feed(&payload(730, "m1", 5), at(t0, 0)), None);
        let menu: Payload =
            serde_json::from_str(r#"{"provider":{"appid":570},"map":{"matchid":"0"}}"#).unwrap();
        assert!(menu.is_dota());
        assert_eq!(t.feed(&menu, at(t0, 1)), None);
        assert!(!payload(730, "m1", 0).is_dota());
    }

    #[test]
    fn the_cfg_points_at_this_computer_and_asks_for_the_player() {
        let text = cfg_text(38517);
        assert!(text.contains("http://127.0.0.1:38517/"));
        assert!(text.contains("\"player\" \"1\""));
    }

    #[test]
    fn the_cfg_goes_into_a_folder_it_creates_beside_the_game_cfgs() {
        let root = std::env::temp_dir().join(format!("sc-dota-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cfg = root
            .join("steamapps")
            .join("common")
            .join("dota 2 beta")
            .join("game")
            .join("dota")
            .join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        let dirs = cfg_dirs(std::slice::from_ref(&root));
        assert_eq!(dirs, vec![cfg.join("gamestate_integration")]);
        assert_eq!(install(&dirs, 38517), 1);
        let file = dirs[0].join(crate::cs2::CFG_NAME);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), cfg_text(38517));
        assert_eq!(remove(&dirs), 1);
        assert!(!file.exists());
        assert!(cfg_dirs(&[root.join("nowhere")]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
