use sc_core::config::{Config, FolderPolicy};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn clip_path(cfg: &Config, app: Option<&str>) -> PathBuf {
    media_path(cfg, app, "mp4")
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn screenshot_path(cfg: &Config, app: Option<&str>) -> PathBuf {
    media_path(cfg, app, "png")
}

fn media_path(cfg: &Config, app: Option<&str>, ext: &str) -> PathBuf {
    let base = cfg.save.resolve_dir();

    let (y, mo, d, h, mi, s) = now();
    let dir = match cfg.save.folder_policy {
        FolderPolicy::Flat => base,
        FolderPolicy::PerDay => base.join(format!("{y:04}-{mo:02}-{d:02}")),
        FolderPolicy::PerApp => base.join(app.unwrap_or("unknown")),
    };

    let time = format!("{h:02}{mi:02}{s:02}");
    let name = cfg
        .save
        .filename_template
        .replace("{date}", &format!("{y:04}{mo:02}{d:02}"))
        .replace("{time}", &time)
        .replace("{app}", app.unwrap_or(""));
    let name = clean(&name);
    let path = dir.join(format!("{name}.{ext}"));
    if !path.exists() {
        return path;
    }
    let timed = path.with_file_name(format!("{name}-{time}.{ext}"));
    if !timed.exists() {
        return timed;
    }
    (2u32..)
        .map(|n| path.with_file_name(format!("{name}-{time}-{n}.{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(timed)
}

fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.chars() {
        let c = if ch.is_ascii_alphanumeric() || ch == '_' {
            ch
        } else {
            '-'
        };
        if c == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(c);
    }
    out.trim_matches(&['-', '_'][..]).to_string()
}

#[cfg(windows)]
pub fn foreground_app() -> Option<String> {
    let exe = crate::win::foreground_exe()?;
    let stem = exe.file_stem()?.to_str()?.to_ascii_lowercase();
    if matches!(stem.as_str(), "explorer" | "scd" | "sc-gui" | "searchhost") {
        return None;
    }
    let app = clean(&stem);
    (!app.is_empty()).then_some(app)
}

#[cfg(not(windows))]
pub fn foreground_app() -> Option<String> {
    let out = std::process::Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let class = v
        .get("class")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())?;
    let app = clean(&class.to_ascii_lowercase());
    (!app.is_empty()).then_some(app)
}

#[cfg(windows)]
fn now() -> (i64, u32, u32, u32, u32, u32) {
    let t = crate::win::local_time();
    (
        t.0 as i64, t.1 as u32, t.2 as u32, t.3 as u32, t.4 as u32, t.5 as u32,
    )
}

#[cfg(not(windows))]
fn now() -> (i64, u32, u32, u32, u32, u32) {
    now_utc()
}

#[cfg_attr(windows, allow(dead_code))]
fn now_utc() -> (i64, u32, u32, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, mo, d) = civil_from_days(days);
    (
        y,
        mo,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

#[cfg_attr(windows, allow(dead_code))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_second_saves_get_distinct_names() {
        let dir = std::env::temp_dir().join(format!("sc-naming-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::default();
        cfg.save.directory = Some(dir.clone());
        cfg.save.folder_policy = FolderPolicy::Flat;
        std::fs::create_dir_all(&dir).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..6 {
            let p = media_path(&cfg, Some("game"), "mp4");
            assert!(seen.insert(p.clone()), "reused {}", p.display());
            std::fs::write(&p, b"x").unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
