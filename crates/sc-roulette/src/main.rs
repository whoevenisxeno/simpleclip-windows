#![cfg_attr(windows, windows_subsystem = "windows")]

use sc_core::store::{self, Manifest};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MANIFEST: &str = include_str!("../manifest.json");
const MIN_SIZE: u64 = 100 * 1024;
const OLD_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
const DEPTH: usize = 4;
const EMPTY: &str = "No clips to spin yet. Save a few first.";

#[derive(Debug, Clone, PartialEq)]
struct Clip {
    path: PathBuf,
    old: bool,
}

fn has_trim_suffix(stem: &str) -> bool {
    stem.rfind("-trim")
        .is_some_and(|i| stem[i + 5..].chars().all(|c| c.is_ascii_digit()))
}

fn has_compress_suffix(stem: &str) -> bool {
    let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    let is_size = |p: &str| p.strip_suffix("mb").is_some_and(digits);
    let parts: Vec<&str> = stem.split('-').collect();
    match parts.as_slice() {
        [.., last] if is_size(last) => true,
        [.., size, n] => is_size(size) && digits(n),
        _ => false,
    }
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4") || e.eq_ignore_ascii_case("mkv"))
}

fn wanted(path: &Path, size: u64) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    is_video(path) && size >= MIN_SIZE && !has_trim_suffix(stem) && !has_compress_suffix(stem)
}

fn walk(dir: &Path, depth: usize, now: SystemTime, out: &mut Vec<Clip>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else {
            continue;
        };
        let path = e.path();
        if meta.is_dir() {
            if depth < DEPTH {
                walk(&path, depth + 1, now, out);
            }
        } else if meta.is_file() && wanted(&path, meta.len()) {
            let age = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .unwrap_or_default();
            out.push(Clip {
                path,
                old: age > OLD_AFTER,
            });
        }
    }
}

fn clips(dir: &Path, now: SystemTime) -> Vec<Clip> {
    let mut out = Vec::new();
    walk(dir, 0, now, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn weight(c: &Clip, old_first: bool) -> u64 {
    if old_first && c.old {
        3
    } else {
        1
    }
}

fn pick(clips: &[Clip], old_first: bool, roll: u64) -> Option<&Clip> {
    let total: u64 = clips.iter().map(|c| weight(c, old_first)).sum();
    if total == 0 {
        return None;
    }
    let mut left = roll % total;
    for c in clips {
        let w = weight(c, old_first);
        if left < w {
            return Some(c);
        }
        left -= w;
    }
    None
}

fn roll() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = (nanos ^ (u64::from(std::process::id()) << 32)) | 1;
    for _ in 0..4 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    x
}

fn reply(choice: Option<&Path>) -> String {
    let v = match choice {
        Some(p) => {
            let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
            json!({ "open": abs.to_string_lossy() })
        }
        None => json!({ "toast": EMPTY }),
    };
    v.to_string()
}

fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST).unwrap_or_default()
}

fn old_first() -> bool {
    let values = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(store::read_settings))
        .unwrap_or_default();
    store::value(&manifest(), &values, "old-first")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn spin(dir: Option<&Path>, old_first: bool, roll: u64) -> String {
    let found = dir.map(|d| clips(d, SystemTime::now())).unwrap_or_default();
    reply(pick(&found, old_first, roll).map(|c| c.path.as_path()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    if has("--version") {
        println!("{VERSION}");
        return;
    }
    if has("--uninstall") {
        return;
    }
    if value("--button").as_deref() != Some("spin") {
        return;
    }
    let dir = value("--clips").map(PathBuf::from);
    println!("{}", spin(dir.as_deref(), old_first(), roll()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn clip(name: &str, old: bool) -> Clip {
        Clip {
            path: PathBuf::from(name),
            old,
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-roulette-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, size: usize, age: Duration) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; size]).unwrap();
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn edited_copies_and_other_files_are_skipped() {
        let big = MIN_SIZE;
        assert!(wanted(Path::new("Valorant 2026-10-01.mp4"), big));
        assert!(wanted(Path::new("clip.MKV"), big));
        assert!(wanted(Path::new("my-trimmed-clip.mp4"), big));
        assert!(wanted(Path::new("10mb tips.mp4"), big));
        assert!(!wanted(Path::new("clip.mp4"), big - 1));
        assert!(!wanted(Path::new("clip.mov"), big));
        assert!(!wanted(Path::new("clip-trim.mp4"), big));
        assert!(!wanted(Path::new("clip-trim2.mp4"), big));
        assert!(!wanted(Path::new("clip-10mb.mp4"), big));
        assert!(!wanted(Path::new("clip-10mb-2.mp4"), big));
        assert!(!wanted(Path::new("clip-trim-25mb.mkv"), big));
    }

    #[test]
    fn old_clips_weigh_three_times_as_much() {
        let list = vec![clip("old", true), clip("new", false)];
        let count = |old_first: bool, name: &str| {
            (0..400)
                .filter(|r| pick(&list, old_first, *r).unwrap().path == Path::new(name))
                .count()
        };
        assert_eq!(count(true, "old"), 300);
        assert_eq!(count(true, "new"), 100);
        assert_eq!(count(false, "old"), 200);
        assert_eq!(count(false, "new"), 200);
        let recent = vec![clip("a", false), clip("b", false)];
        assert!(pick(&recent, true, 7).is_some());
        assert!(pick(&[], true, 7).is_none());
    }

    #[test]
    fn the_reply_is_one_json_line() {
        let open: Value = serde_json::from_str(&reply(Some(Path::new("a.mp4")))).unwrap();
        let path = PathBuf::from(open["open"].as_str().unwrap());
        assert!(path.is_absolute() && path.ends_with("a.mp4"));
        let toast = reply(None);
        assert!(!toast.contains('\n'));
        assert_eq!(
            serde_json::from_str::<Value>(&toast).unwrap()["toast"],
            EMPTY
        );
    }

    #[test]
    fn the_manifest_parses_with_its_toggle() {
        let m = manifest();
        assert_eq!(m.name, "roulette");
        assert_eq!(m.version, VERSION);
        assert_eq!(m.buttons[0].id, "spin");
        let mut v = serde_json::Map::new();
        assert_eq!(store::value(&m, &v, "old-first"), Some(Value::Bool(true)));
        v.insert("old-first".into(), Value::Bool(false));
        assert_eq!(store::value(&m, &v, "old-first"), Some(Value::Bool(false)));
    }

    #[test]
    fn walking_finds_clips_with_ages_and_respects_depth() {
        let dir = temp("walk");
        let week = Duration::from_secs(8 * 24 * 3600);
        put(&dir.join("old.mp4"), 200_000, week);
        put(&dir.join("game/new.mkv"), 200_000, Duration::ZERO);
        put(&dir.join("game/new-trim.mkv"), 200_000, Duration::ZERO);
        put(&dir.join("game/tiny.mp4"), 10, Duration::ZERO);
        put(&dir.join("a/b/c/d/deep.mp4"), 200_000, Duration::ZERO);
        put(&dir.join("a/b/c/d/e/deeper.mp4"), 200_000, Duration::ZERO);
        let found = clips(&dir, SystemTime::now());
        let names: Vec<_> = found
            .iter()
            .map(|c| (c.path.file_name().unwrap().to_str().unwrap(), c.old))
            .collect();
        assert_eq!(
            names,
            vec![("deep.mp4", false), ("new.mkv", false), ("old.mp4", true)]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
