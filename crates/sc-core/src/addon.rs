use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const EVENTS_EXE: &str = "sc-events.exe";
pub const EVENTS_ZIP_SUFFIX: &str = "-events.zip";
pub const MANIFEST: &str = "manifest.json";
pub const STATUS: &str = "status.json";

pub fn events_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "SimpleClip")
        .map(|d| d.data_local_dir().join("addons").join("events"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
}

pub fn installed_version(dir: &Path) -> Option<String> {
    if !dir.join(EVENTS_EXE).exists() {
        return None;
    }
    let text = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    let m: Manifest = serde_json::from_str(&text).ok()?;
    (m.name == "events").then_some(m.version)
}

pub fn write_manifest(dir: &Path, version: &str) -> std::io::Result<()> {
    let m = Manifest {
        name: "events".into(),
        version: version.into(),
    };
    std::fs::write(
        dir.join(MANIFEST),
        serde_json::to_string_pretty(&m).unwrap_or_default(),
    )
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EventsStatus {
    pub version: String,
    pub cs2: String,
    pub league: String,
    pub r6: String,
    pub dota2: String,
    pub last_event: Option<String>,
    pub updated: u64,
}

pub fn read_status(dir: &Path) -> Option<EventsStatus> {
    let text = std::fs::read_to_string(dir.join(STATUS)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_status(dir: &Path, status: &EventsStatus) -> std::io::Result<()> {
    let tmp = dir.join("status.json.tmp");
    std::fs::write(&tmp, serde_json::to_string(status).unwrap_or_default())?;
    std::fs::rename(&tmp, dir.join(STATUS))
}

pub fn status_is_fresh(status: &EventsStatus, now_secs: u64) -> bool {
    now_secs.saturating_sub(status.updated) <= 8
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-addon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_add_on_counts_as_installed_only_with_its_program_and_manifest() {
        let dir = temp("installed");
        assert_eq!(installed_version(&dir), None);
        write_manifest(&dir, "6.8.0").unwrap();
        assert_eq!(installed_version(&dir), None);
        std::fs::write(dir.join(EVENTS_EXE), b"x").unwrap();
        assert_eq!(installed_version(&dir).as_deref(), Some("6.8.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_for_something_else_is_ignored() {
        let dir = temp("other");
        std::fs::write(dir.join(EVENTS_EXE), b"x").unwrap();
        std::fs::write(dir.join(MANIFEST), r#"{"name":"voice","version":"1.0.0"}"#).unwrap();
        assert_eq!(installed_version(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_status_round_trips_and_goes_stale() {
        let dir = temp("status");
        assert!(read_status(&dir).is_none());
        let s = EventsStatus {
            version: "6.8.0".into(),
            cs2: "connected".into(),
            league: "waiting".into(),
            last_event: Some("Double kill".into()),
            updated: 1000,
            ..Default::default()
        };
        write_status(&dir, &s).unwrap();
        assert_eq!(read_status(&dir), Some(s.clone()));
        assert!(status_is_fresh(&s, 1005));
        assert!(!status_is_fresh(&s, 1020));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_half_written_status_is_not_trusted() {
        let dir = temp("torn");
        std::fs::write(dir.join(STATUS), "{\"version\":").unwrap();
        assert!(read_status(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
