use sc_core::config::{Config, FolderPolicy};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolve the full output path for a saved clip from config: base directory,
/// folder policy (flat / per-day / per-app), and the filename template.
pub fn clip_path(cfg: &Config, app: Option<&str>) -> PathBuf {
    let base = cfg
        .save
        .directory
        .clone()
        .or_else(|| directories::UserDirs::new().and_then(|d| d.video_dir().map(PathBuf::from)))
        .unwrap_or_else(|| PathBuf::from("."));

    let (y, mo, d, h, mi, s) = now_utc();
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
    let path = dir.join(format!("{name}.mp4"));
    // Same-day clips without a time in the name would collide; add one if needed.
    if path.exists() {
        return path.with_file_name(format!("{name}-{time}.mp4"));
    }
    path
}

/// Collapse anything that isn't alphanumeric or `_` into single dashes, so an
/// empty {app} in "sc-{app}-{date}" yields "sc-<date>", not "sc--<date>".
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

/// Best-effort foreground app/game name for the {app} token (Hyprland).
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

/// Howard Hinnant's days-to-civil-date algorithm (days since 1970-01-01 -> Y/M/D).
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
