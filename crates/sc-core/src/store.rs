use crate::update::{curl, is_newer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/whoevenisxeno/simpleclip-windows/addons/catalog.json";
pub const CATALOG_API: &str =
    "https://api.github.com/repos/whoevenisxeno/simpleclip-windows/contents/catalog.json?ref=addons";
pub const PACKAGE_PREFIX: &str =
    "https://raw.githubusercontent.com/whoevenisxeno/simpleclip-windows/addons/packages/";
pub const MANIFEST: &str = "manifest.json";
pub const SETTINGS: &str = "settings.json";
pub const ENABLED: &str = "enabled";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub about: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub exe: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub min_app: String,
    pub background: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<Button>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Setting>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub place: String,
    pub icon: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub hint: String,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Kind {
    Toggle {
        default: bool,
    },
    Choice {
        options: Vec<String>,
        default: String,
    },
    Number {
        min: i64,
        max: i64,
        default: i64,
        #[serde(default)]
        unit: String,
    },
}

impl Manifest {
    pub fn exe_name(&self) -> String {
        if self.exe.is_empty() {
            format!("sc-{}.exe", self.name)
        } else {
            self.exe.clone()
        }
    }

    pub fn legacy(&self) -> bool {
        self.exe.is_empty()
    }

    pub fn title(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        if self.name == "events" {
            return "Game events".into();
        }
        self.name.clone()
    }

    pub fn setting(&self, key: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.key == key)
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SC_ADDONS_DIR") {
        return Some(PathBuf::from(p));
    }
    directories::ProjectDirs::from("", "", "SimpleClip").map(|d| d.data_local_dir().join("addons"))
}

pub fn dir_of(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

pub fn staged_of(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{id}.new"))
}

fn plain_file(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != ".."
        && name.to_ascii_lowercase().ends_with(".exe")
}

pub fn part_of(root: &Path, entry: &Entry) -> PathBuf {
    root.join(format!("{}-{}.zip.part", entry.id, entry.version))
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    let m: Manifest = serde_json::from_str(&text).ok()?;
    if !valid_id(&m.name) || !plain_file(&m.exe_name()) || !dir.join(m.exe_name()).exists() {
        return None;
    }
    Some(m)
}

pub fn installed(root: &Path) -> Vec<(PathBuf, Manifest)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, Manifest)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            let m = read_manifest(&p)?;
            (m.name == name).then_some((p, m))
        })
        .collect();
    out.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    out
}

pub fn read_settings(dir: &Path) -> Map<String, Value> {
    std::fs::read_to_string(dir.join(SETTINGS))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

pub fn write_settings(dir: &Path, values: &Map<String, Value>) -> std::io::Result<()> {
    let tmp = dir.join("settings.json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(values).unwrap_or_default(),
    )?;
    std::fs::rename(&tmp, dir.join(SETTINGS))
}

pub fn is_enabled(values: &Map<String, Value>) -> bool {
    values.get(ENABLED).and_then(Value::as_bool).unwrap_or(true)
}

pub fn value(m: &Manifest, values: &Map<String, Value>, key: &str) -> Option<Value> {
    let s = m.setting(key)?;
    let got = values.get(key);
    Some(match &s.kind {
        Kind::Toggle { default } => Value::Bool(got.and_then(Value::as_bool).unwrap_or(*default)),
        Kind::Choice { options, default } => Value::String(
            got.and_then(Value::as_str)
                .filter(|v| options.iter().any(|o| o == v))
                .unwrap_or(default)
                .to_string(),
        ),
        Kind::Number {
            min, max, default, ..
        } => Value::from(
            got.and_then(Value::as_i64)
                .unwrap_or(*default)
                .clamp(*min, *max),
        ),
    })
}

pub const STATUS: &str = "status.json";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    pub line: String,
    pub updated: u64,
}

pub fn read_status(dir: &Path) -> Option<Status> {
    let text = std::fs::read_to_string(dir.join(STATUS)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_status(dir: &Path, line: &str) -> std::io::Result<()> {
    let s = Status {
        line: line.into(),
        updated: crate::addon::now_secs(),
    };
    let tmp = dir.join("status.json.tmp");
    std::fs::write(&tmp, serde_json::to_string(&s).unwrap_or_default())?;
    std::fs::rename(&tmp, dir.join(STATUS))
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub addons: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub about: String,
    pub version: String,
    pub min_app: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

impl Entry {
    fn ok(&self) -> bool {
        valid_id(&self.id)
            && !self.version.is_empty()
            && self.url.starts_with(PACKAGE_PREFIX)
            && self.url.ends_with(".zip")
            && !self.url[PACKAGE_PREFIX.len()..].contains("..")
            && self.sha256.len() == 64
            && self.sha256.chars().all(|c| c.is_ascii_hexdigit())
    }

    pub fn fits(&self, app: &str) -> bool {
        self.min_app.is_empty() || !is_newer(&self.min_app, app)
    }
}

pub fn parse_catalog(bytes: &[u8]) -> Result<Catalog, String> {
    let mut c: Catalog = serde_json::from_slice(bytes)
        .map_err(|_| "the add-on list could not be read".to_string())?;
    c.addons.retain(Entry::ok);
    let mut seen = std::collections::HashSet::new();
    c.addons.retain(|e| seen.insert(e.id.clone()));
    Ok(c)
}

pub fn fetch_catalog() -> Result<Catalog, String> {
    if let Ok(local) = std::env::var("SC_CATALOG") {
        let bytes = std::fs::read(&local).map_err(|e| format!("couldn't read {local}: {e}"))?;
        return parse_catalog(&bytes);
    }
    curl(&["-H", "Accept: application/vnd.github.raw", CATALOG_API])
        .and_then(|b| parse_catalog(&b))
        .or_else(|_| curl(&[CATALOG_URL]).and_then(|b| parse_catalog(&b)))
}

pub fn needs_update(have: &Manifest, entry: &Entry) -> bool {
    have.legacy() || is_newer(&entry.version, &have.version)
}

pub fn sha256_of(path: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new("certutil");
    cmd.arg("-hashfile").arg(path).arg("SHA256");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().replace(' ', ""))
        .find(|l| l.len() == 64 && l.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|l| l.to_ascii_lowercase())
}

pub fn download(entry: &Entry, dest: &Path) -> Result<(), String> {
    let local = std::env::var("SC_ADDON_SOURCE").ok().map(PathBuf::from);
    match local {
        Some(dir) => {
            let name = entry.url.rsplit('/').next().unwrap_or_default();
            std::fs::copy(dir.join(name), dest)
                .map(|_| ())
                .map_err(|e| format!("couldn't copy the add-on: {e}"))?;
        }
        None => {
            let d = dest.to_string_lossy().to_string();
            curl(&["--max-time", "600", "-o", &d, &entry.url])?;
        }
    }
    match sha256_of(dest) {
        Some(h) if h.eq_ignore_ascii_case(&entry.sha256) => Ok(()),
        _ => {
            let _ = std::fs::remove_file(dest);
            Err("the download did not match its checksum, so it was not installed".into())
        }
    }
}

pub fn unpack(zip: &Path, stage: &Path, entry: &Entry) -> Result<Manifest, String> {
    let _ = std::fs::remove_dir_all(stage);
    std::fs::create_dir_all(stage).map_err(|e| format!("couldn't prepare the add-on: {e}"))?;
    let mut cmd = std::process::Command::new("tar");
    cmd.arg("-xf").arg(zip).arg("-C").arg(stage);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    let m = ok.then(|| read_manifest(stage)).flatten();
    match m {
        Some(m) if m.name == entry.id && m.version == entry.version => Ok(m),
        _ => {
            let _ = std::fs::remove_dir_all(stage);
            Err("the add-on package was incomplete, so it was not installed".into())
        }
    }
}

pub fn stage(root: &Path, entry: &Entry) -> Result<Manifest, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("couldn't prepare the add-on: {e}"))?;
    let zip = part_of(root, entry);
    download(entry, &zip)?;
    let part = root.join(format!("{}.part", entry.id));
    let result = unpack(&zip, &part, entry);
    let _ = std::fs::remove_file(&zip);
    let m = result?;
    let new = staged_of(root, &entry.id);
    let _ = std::fs::remove_dir_all(&new);
    if let Err(e) = std::fs::rename(&part, &new) {
        let _ = std::fs::remove_dir_all(&part);
        return Err(format!("couldn't prepare the add-on: {e}"));
    }
    Ok(m)
}

fn staged_manifest(root: &Path, id: &str) -> Option<Manifest> {
    read_manifest(&staged_of(root, id)).filter(|m| m.name == id)
}

pub fn has_staged(root: &Path, id: &str) -> bool {
    staged_manifest(root, id).is_some()
}

pub fn apply(root: &Path, id: &str) -> Result<bool, String> {
    let new = staged_of(root, id);
    if !new.exists() {
        return Ok(false);
    }
    if staged_manifest(root, id).is_none() {
        let _ = std::fs::remove_dir_all(&new);
        return Ok(false);
    }
    let live = dir_of(root, id);
    let old = root.join(format!("{id}.old"));
    let _ = std::fs::remove_dir_all(&old);
    let kept = read_settings(&live);
    if live.exists() {
        std::fs::rename(&live, &old).map_err(|_| "the add-on is still running".to_string())?;
    }
    if let Err(e) = std::fs::rename(&new, &live) {
        let _ = std::fs::rename(&old, &live);
        return Err(format!("couldn't finish the update: {e}"));
    }
    if !kept.is_empty() {
        let _ = write_settings(&live, &kept);
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(dir: &Path, manifest: &str, exe: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest).unwrap();
        std::fs::write(dir.join(exe), b"x").unwrap();
    }

    fn entry(id: &str, version: &str) -> Entry {
        Entry {
            id: id.into(),
            version: version.into(),
            url: format!("{PACKAGE_PREFIX}{id}-{version}.zip"),
            sha256: "a".repeat(64),
            ..Default::default()
        }
    }

    #[test]
    fn an_old_events_manifest_still_reads_and_counts_as_outdated() {
        let dir = temp("legacy");
        let events = dir.join("events");
        put(
            &events,
            r#"{"name":"events","version":"6.9.0"}"#,
            "sc-events.exe",
        );
        let m = read_manifest(&events).unwrap();
        assert!(m.legacy());
        assert_eq!(m.title(), "Game events");
        assert!(needs_update(&m, &entry("events", "1.0.0")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_manifest_with_buttons_and_settings_reads() {
        let dir = temp("full");
        let voice = dir.join("voice");
        put(
            &voice,
            r#"{"name":"voice","version":"1.2.0","title":"Clip that","exe":"sc-voice.exe","background":true,
               "buttons":[{"id":"spin","label":"Spin","place":"home","icon":"dice"}],
               "settings":[{"key":"phrase","label":"Phrase","kind":"choice","options":["clip that","save that"],"default":"clip that"},
                           {"key":"beep","label":"Beep","kind":"toggle","default":true},
                           {"key":"secs","label":"Length","kind":"number","min":5,"max":120,"default":30,"unit":"s"}]}"#,
            "sc-voice.exe",
        );
        let m = read_manifest(&voice).unwrap();
        assert!(!m.legacy() && m.background);
        assert_eq!(m.buttons[0].place, "home");
        let mut v = Map::new();
        assert_eq!(value(&m, &v, "phrase"), Some(Value::from("clip that")));
        v.insert("phrase".into(), Value::from("nonsense"));
        v.insert("secs".into(), Value::from(500));
        v.insert("beep".into(), Value::from(false));
        assert_eq!(value(&m, &v, "phrase"), Some(Value::from("clip that")));
        assert_eq!(value(&m, &v, "secs"), Some(Value::from(120)));
        assert_eq!(value(&m, &v, "beep"), Some(Value::from(false)));
        assert_eq!(value(&m, &v, "missing"), None);
        assert_eq!(installed(&dir).len(), 1);
        assert!(!needs_update(&m, &entry("voice", "1.2.0")));
        assert!(needs_update(&m, &entry("voice", "1.10.0")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folders_that_do_not_match_their_manifest_are_ignored() {
        let dir = temp("mismatch");
        put(
            &dir.join("voice"),
            r#"{"name":"events","version":"1.0.0","exe":"a.exe"}"#,
            "a.exe",
        );
        put(
            &dir.join("voice.new"),
            r#"{"name":"voice","version":"1.0.0","exe":"a.exe"}"#,
            "a.exe",
        );
        put(
            &dir.join("Bad Name"),
            r#"{"name":"Bad Name","version":"1.0.0","exe":"a.exe"}"#,
            "a.exe",
        );
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        assert!(installed(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_and_enabled_defaults_on() {
        let dir = temp("settings");
        assert!(is_enabled(&read_settings(&dir)));
        let mut v = Map::new();
        v.insert(ENABLED.into(), Value::Bool(false));
        write_settings(&dir, &v).unwrap();
        assert!(!is_enabled(&read_settings(&dir)));
        std::fs::write(dir.join(SETTINGS), "{broken").unwrap();
        assert!(read_settings(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_catalog_keeps_only_safe_entries() {
        let good = entry("voice", "1.0.0");
        let mut elsewhere = entry("roulette", "1.0.0");
        elsewhere.url = "https://evil.example/roulette.zip".into();
        let mut no_hash = entry("x", "1.0.0");
        no_hash.sha256 = "abc".into();
        let mut sneaky = entry("y", "1.0.0");
        sneaky.url = format!("{PACKAGE_PREFIX}../../main/y.zip");
        let mut bad_id = entry("z", "1.0.0");
        bad_id.id = "../z".into();
        let dup = entry("voice", "9.0.0");
        let json = serde_json::to_vec(&Catalog {
            addons: vec![good.clone(), elsewhere, no_hash, sneaky, bad_id, dup],
        })
        .unwrap();
        let c = parse_catalog(&json).unwrap();
        assert_eq!(c.addons, vec![good]);
        assert!(parse_catalog(b"<html>").is_err());
        assert_eq!(parse_catalog(b"{}").unwrap().addons.len(), 0);
    }

    #[test]
    fn an_add_on_needing_a_newer_app_does_not_fit() {
        let mut e = entry("voice", "1.0.0");
        assert!(e.fits("6.9.0"));
        e.min_app = "6.10.0".into();
        assert!(!e.fits("6.9.0"));
        assert!(e.fits("6.10.0"));
        assert!(e.fits("7.0.0"));
    }

    #[test]
    fn applying_a_staged_update_swaps_folders_and_keeps_settings() {
        let dir = temp("apply");
        put(
            &dir.join("voice"),
            r#"{"name":"voice","version":"1.0.0","exe":"v.exe"}"#,
            "v.exe",
        );
        let mut v = Map::new();
        v.insert("phrase".into(), Value::from("save that"));
        write_settings(&dir.join("voice"), &v).unwrap();
        assert_eq!(apply(&dir, "voice"), Ok(false));
        put(
            &staged_of(&dir, "voice"),
            r#"{"name":"voice","version":"1.1.0","exe":"v.exe"}"#,
            "v.exe",
        );
        assert!(has_staged(&dir, "voice"));
        assert_eq!(apply(&dir, "voice"), Ok(true));
        assert_eq!(read_manifest(&dir.join("voice")).unwrap().version, "1.1.0");
        assert_eq!(read_settings(&dir.join("voice")), v);
        assert!(!staged_of(&dir, "voice").exists());
        assert!(!dir.join("voice.old").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_local_package_installs_end_to_end_and_a_tampered_one_does_not() {
        let dir = temp("stage");
        let src = dir.join("src");
        put(
            &src,
            r#"{"name":"voice","version":"1.0.0","exe":"v.exe"}"#,
            "v.exe",
        );
        let pkgs = dir.join("pkgs");
        std::fs::create_dir_all(&pkgs).unwrap();
        let zip = pkgs.join("voice-1.0.0.zip");
        let mut cmd = std::process::Command::new("tar");
        cmd.args(["-a", "-c", "-f"])
            .arg(&zip)
            .arg("-C")
            .arg(&src)
            .args([MANIFEST, "v.exe"]);
        assert!(cmd.status().unwrap().success());
        let mut e = entry("voice", "1.0.0");
        e.sha256 = sha256_of(&zip).unwrap();
        std::env::set_var("SC_ADDON_SOURCE", &pkgs);
        let root = dir.join("root");
        let m = stage(&root, &e).unwrap();
        assert_eq!(m.version, "1.0.0");
        assert_eq!(apply(&root, "voice"), Ok(true));
        assert_eq!(installed(&root).len(), 1);
        e.sha256 = "b".repeat(64);
        assert!(stage(&root, &e).unwrap_err().contains("checksum"));
        e.sha256 = sha256_of(&zip).unwrap();
        e.version = "2.0.0".into();
        std::fs::copy(&zip, pkgs.join("voice-2.0.0.zip")).unwrap();
        e.url = format!("{PACKAGE_PREFIX}voice-2.0.0.zip");
        assert!(stage(&root, &e).unwrap_err().contains("incomplete"));
        std::env::remove_var("SC_ADDON_SOURCE");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_pointing_outside_its_folder_is_refused() {
        let dir = temp("escape");
        let a = dir.join("a");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(dir.join("x.exe"), b"x").unwrap();
        std::fs::write(
            a.join(MANIFEST),
            r#"{"name":"a","version":"1.0.0","exe":"..\\x.exe"}"#,
        )
        .unwrap();
        assert!(read_manifest(&a).is_none());
        std::fs::write(a.join("run.bat"), b"x").unwrap();
        std::fs::write(
            a.join(MANIFEST),
            r#"{"name":"a","version":"1.0.0","exe":"run.bat"}"#,
        )
        .unwrap();
        assert!(read_manifest(&a).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_staged_folder_for_another_add_on_is_thrown_away() {
        let dir = temp("wrong-stage");
        put(
            &staged_of(&dir, "voice"),
            r#"{"name":"events","version":"9.0.0","exe":"v.exe"}"#,
            "v.exe",
        );
        assert!(!has_staged(&dir, "voice"));
        assert_eq!(apply(&dir, "voice"), Ok(false));
        assert!(!staged_of(&dir, "voice").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "reaches GitHub"]
    fn the_published_catalog_reads_from_the_api_and_the_mirror() {
        let api = curl(&["-H", "Accept: application/vnd.github.raw", CATALOG_API]).unwrap();
        let raw = curl(&[CATALOG_URL]).unwrap();
        assert!(!parse_catalog(&api).unwrap().addons.is_empty());
        assert!(!parse_catalog(&raw).unwrap().addons.is_empty());
        assert!(!fetch_catalog().unwrap().addons.is_empty());
    }
}
