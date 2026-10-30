use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const PORT: u16 = 38517;
pub const CFG_NAME: &str = "gamestate_integration_simpleclip.cfg";
const MAX_BODY: usize = 1 << 20;

#[derive(Debug, Default, Deserialize)]
pub struct Payload {
    provider: Option<Provider>,
    player: Option<Player>,
    map: Option<MapInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct Provider {
    steamid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Player {
    steamid: Option<String>,
    state: Option<PlayerState>,
}

#[derive(Debug, Default, Deserialize)]
struct PlayerState {
    round_kills: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct MapInfo {
    round: Option<i64>,
}

impl Payload {
    fn local_state(&self) -> Option<(Option<i64>, u32)> {
        let me = self.provider.as_ref()?.steamid.as_deref()?;
        let player = self.player.as_ref()?;
        if player.steamid.as_deref()? != me {
            return None;
        }
        let kills = player.state.as_ref()?.round_kills?;
        Some((self.map.as_ref().and_then(|m| m.round), kills))
    }
}

#[derive(Default)]
pub struct Tracker {
    round: Option<i64>,
    kills: u32,
    seen: bool,
}

impl Tracker {
    pub fn feed(&mut self, p: &Payload) -> Option<u32> {
        let (round, kills) = p.local_state()?;
        if !self.seen || round != self.round {
            self.seen = true;
            self.round = round;
            self.kills = kills;
            return None;
        }
        let rose = kills > self.kills;
        self.kills = kills;
        rose.then_some(kills)
    }
}

pub fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port))
}

fn read_body(stream: &mut TcpStream) -> Option<Vec<u8>> {
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if data.len() > 16 * 1024 {
            return None;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_ascii_lowercase();
    let wanted: usize = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    if wanted > MAX_BODY {
        return None;
    }
    while data.len() < head_end + wanted {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
    }
    Some(data[head_end..head_end + wanted].to_vec())
}

pub fn serve(listener: TcpListener, mut on_payload: impl FnMut(Payload)) {
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        let body = read_body(&mut stream);
        let _ =
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        if let Some(payload) = body.and_then(|b| serde_json::from_slice::<Payload>(&b).ok()) {
            on_payload(payload);
        }
    }
}

pub fn cfg_text(port: u16) -> String {
    format!(
        "\"SimpleClip\"\n{{\n    \"uri\" \"http://127.0.0.1:{port}/\"\n    \"timeout\" \"5.0\"\n    \"buffer\" \"0.1\"\n    \"throttle\" \"0.1\"\n    \"heartbeat\" \"10.0\"\n    \"data\"\n    {{\n        \"provider\" \"1\"\n        \"map\" \"1\"\n        \"player_id\" \"1\"\n        \"player_state\" \"1\"\n    }}\n}}\n"
    )
}

pub fn parse_library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("\"path\"") {
                return None;
            }
            let value = line.split('"').nth(3)?;
            Some(PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

pub fn steam_libraries(steam_root: &Path) -> Vec<PathBuf> {
    let mut libraries = vec![steam_root.to_path_buf()];
    let vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(vdf) {
        for path in parse_library_paths(&text) {
            if !libraries.contains(&path) {
                libraries.push(path);
            }
        }
    }
    libraries
}

pub fn cfg_dirs(libraries: &[PathBuf]) -> Vec<PathBuf> {
    libraries
        .iter()
        .map(|l| {
            l.join("steamapps")
                .join("common")
                .join("Counter-Strike Global Offensive")
                .join("game")
                .join("csgo")
                .join("cfg")
        })
        .filter(|d| d.is_dir())
        .collect()
}

#[cfg(windows)]
pub fn steam_root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path.replace('/', "\\")))
}

#[cfg(not(windows))]
pub fn steam_root() -> Option<PathBuf> {
    None
}

pub fn locate() -> Vec<PathBuf> {
    steam_root()
        .map(|root| cfg_dirs(&steam_libraries(&root)))
        .unwrap_or_default()
}

pub fn install(dirs: &[PathBuf], port: u16) -> usize {
    let text = cfg_text(port);
    dirs.iter()
        .filter(|d| {
            let path = d.join(CFG_NAME);
            std::fs::read_to_string(&path).is_ok_and(|have| have == text)
                || std::fs::write(&path, &text).is_ok()
        })
        .count()
}

pub fn remove(dirs: &[PathBuf]) -> usize {
    dirs.iter()
        .filter(|d| std::fs::remove_file(d.join(CFG_NAME)).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(me: &str, who: &str, round: i64, kills: u32) -> Payload {
        serde_json::from_str(&format!(
            r#"{{"provider":{{"name":"Counter-Strike 2","steamid":"{me}"}},
                "map":{{"name":"de_mirage","round":{round}}},
                "player":{{"steamid":"{who}","name":"x","state":{{"health":100,"round_kills":{kills}}}}}}}"#
        ))
        .unwrap()
    }

    const ME: &str = "76561198000000001";

    #[test]
    fn a_rising_kill_count_is_reported_with_the_new_total() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(&payload(ME, ME, 3, 0)), None);
        assert_eq!(t.feed(&payload(ME, ME, 3, 0)), None);
        assert_eq!(t.feed(&payload(ME, ME, 3, 1)), Some(1));
        assert_eq!(t.feed(&payload(ME, ME, 3, 1)), None);
        assert_eq!(t.feed(&payload(ME, ME, 3, 2)), Some(2));
        assert_eq!(t.feed(&payload(ME, ME, 3, 3)), Some(3));
    }

    #[test]
    fn a_new_round_starts_counting_again() {
        let mut t = Tracker::default();
        t.feed(&payload(ME, ME, 3, 0));
        t.feed(&payload(ME, ME, 3, 2));
        assert_eq!(t.feed(&payload(ME, ME, 4, 2)), None);
        assert_eq!(t.feed(&payload(ME, ME, 4, 0)), None);
        assert_eq!(t.feed(&payload(ME, ME, 4, 1)), Some(1));
    }

    #[test]
    fn joining_in_the_middle_of_a_round_does_not_trigger() {
        let mut t = Tracker::default();
        assert_eq!(t.feed(&payload(ME, ME, 9, 3)), None);
        assert_eq!(t.feed(&payload(ME, ME, 9, 3)), None);
        assert_eq!(t.feed(&payload(ME, ME, 9, 4)), Some(4));
    }

    #[test]
    fn watching_someone_else_never_triggers() {
        let mut t = Tracker::default();
        let other = "76561198000000002";
        assert_eq!(t.feed(&payload(ME, other, 3, 0)), None);
        assert_eq!(t.feed(&payload(ME, other, 3, 5)), None);
    }

    #[test]
    fn payloads_without_a_player_are_ignored() {
        let mut t = Tracker::default();
        let menu: Payload =
            serde_json::from_str(r#"{"provider":{"steamid":"1"},"map":{"round":1}}"#).unwrap();
        assert_eq!(t.feed(&menu), None);
        let empty: Payload = serde_json::from_str("{}").unwrap();
        assert_eq!(t.feed(&empty), None);
    }

    fn post(port: u16, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
        reply
    }

    #[test]
    fn the_server_answers_the_game_and_hands_over_each_payload() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || serve(listener, move |p| tx.send(p).unwrap()));
        let body = format!(
            r#"{{"provider":{{"steamid":"{ME}"}},"map":{{"round":5}},"player":{{"steamid":"{ME}","state":{{"round_kills":2}}}}}}"#
        );
        let reply = post(port, &body);
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(got.local_state(), Some((Some(5), 2)));
        let reply = post(port, "not json at all");
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        let reply = post(port, &body);
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
    }

    #[test]
    fn a_game_that_sends_a_huge_body_is_turned_away() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || serve(listener, move |p| tx.send(p).unwrap()));
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(b"POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n{}")
            .unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }

    #[test]
    fn the_cfg_points_the_game_at_this_computer_only() {
        let text = cfg_text(PORT);
        assert!(text.contains(&format!("http://127.0.0.1:{PORT}/")));
        assert!(text.contains("\"player_state\" \"1\""));
        assert!(!text.contains("player_weapons"));
    }

    #[test]
    fn steam_library_folders_are_read_from_the_vdf() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";
        assert_eq!(
            parse_library_paths(vdf),
            vec![
                PathBuf::from("C:\\Program Files (x86)\\Steam"),
                PathBuf::from("D:\\SteamLibrary")
            ]
        );
    }

    fn fake_steam(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sc-steam-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        root
    }

    fn game_cfg(root: &Path) -> PathBuf {
        let dir = root
            .join("steamapps")
            .join("common")
            .join("Counter-Strike Global Offensive")
            .join("game")
            .join("csgo")
            .join("cfg");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_cfg_is_written_into_each_library_that_has_the_game() {
        let root = fake_steam("install");
        let other = fake_steam("install-other");
        let dir = game_cfg(&other);
        let escaped = other.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(
            root.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{escaped}\"\n\t}}\n}}\n"
            ),
        )
        .unwrap();
        let libraries = steam_libraries(&root);
        assert_eq!(libraries.len(), 2);
        let dirs = cfg_dirs(&libraries);
        assert_eq!(dirs, vec![dir.clone()]);
        assert_eq!(install(&dirs, PORT), 1);
        assert_eq!(
            std::fs::read_to_string(dir.join(CFG_NAME)).unwrap(),
            cfg_text(PORT)
        );
        assert_eq!(install(&dirs, PORT), 1);
        assert_eq!(remove(&dirs), 1);
        assert!(!dir.join(CFG_NAME).exists());
        assert_eq!(remove(&dirs), 0);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn a_steam_without_the_game_gets_nothing() {
        let root = fake_steam("none");
        assert!(cfg_dirs(&steam_libraries(&root)).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
