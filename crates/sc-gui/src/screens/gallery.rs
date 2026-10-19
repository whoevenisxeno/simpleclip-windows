use crate::app::{ScApp, Screen};
use crate::{theme, widgets};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
struct Clip {
    path: PathBuf,
    name: String,
    modified: SystemTime,
    bytes: u64,
    is_image: bool,
}

/// Cached directory scan, so the gallery doesn't re-walk the disk on every
/// repaint (the status poller repaints once a second).
#[derive(Default)]
pub struct Cache {
    clips: Vec<Clip>,
    dir: PathBuf,
    scanned: Option<Instant>,
}

const RESCAN_EVERY: Duration = Duration::from_secs(3);

fn save_dir(app: &ScApp) -> PathBuf {
    app.cfg.save.resolve_dir()
}

fn scan(dir: &Path) -> Vec<Clip> {
    let mut clips = Vec::new();
    let walk = walkdir(dir);
    for path in walk {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let is_image = ext == "png";
        if !matches!(ext.as_str(), "mp4" | "mkv" | "png") {
            continue;
        }
        let meta = path.metadata().ok();
        let modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("clip")
            .to_string();
        clips.push(Clip {
            path,
            name,
            modified,
            bytes: meta.map(|m| m.len()).unwrap_or(0),
            is_image,
        });
    }
    clips.sort_by_key(|c| std::cmp::Reverse(c.modified));
    clips
}

/// Small recursive directory walk (max 3 levels) so per-day / per-app subfolders
/// are picked up without pulling in an extra crate.
fn walkdir(root: &Path) -> Vec<PathBuf> {
    fn go(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
        if depth > 3 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                go(&path, depth + 1, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    go(root, 0, &mut out);
    out
}

/// "Mon 19 Oct 2026" in the user's local time zone.
fn day_label(t: SystemTime) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let utc = time::OffsetDateTime::from(t);
    let local = time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc);
    format!(
        "{} {} {} {}",
        DAYS[local.weekday().number_days_from_monday() as usize],
        local.day(),
        MONTHS[local.month() as usize - 1],
        local.year()
    )
}

fn time_label(t: SystemTime) -> String {
    let utc = time::OffsetDateTime::from(t);
    let local = time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc);
    format!("{:02}:{:02}", local.hour(), local.minute())
}

fn size_label(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= 1024.0 * MB {
        format!("{:.1} GB", bytes as f64 / (1024.0 * MB))
    } else {
        format!("{:.1} MB", bytes as f64 / MB)
    }
}

fn open_path(path: &Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// Open the folder with the file selected.
fn reveal(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(parent) = path.parent() {
        open_path(parent);
    }
}

/// Send to the Recycle Bin where there is one, so a mis-click isn't permanent.
fn delete(path: &Path) -> bool {
    #[cfg(windows)]
    if trash::delete(path).is_ok() {
        return true;
    }
    std::fs::remove_file(path).is_ok()
}

pub fn view(app: &mut ScApp, ui: &mut egui::Ui) {
    let dir = save_dir(app);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.heading("Gallery");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Open folder").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                open_path(&dir);
            }
            if ui.button("Refresh").clicked() {
                app.gallery.scanned = None;
            }
        });
    });
    ui.colored_label(theme::MUTED, dir.display().to_string());
    ui.separator();

    let stale = app.gallery.dir != dir
        || app
            .gallery
            .scanned
            .is_none_or(|t| t.elapsed() > RESCAN_EVERY);
    if stale {
        app.gallery.clips = scan(&dir);
        app.gallery.dir = dir;
        app.gallery.scanned = Some(Instant::now());
    }

    if app.gallery.clips.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.colored_label(
                theme::MUTED,
                "No clips yet. Hit your save hotkey during a session.",
            );
        });
        return;
    }
    let clips = app.gallery.clips.clone();
    render_groups(app, ui, clips);
}

fn render_groups(app: &mut ScApp, ui: &mut egui::Ui, clips: Vec<Clip>) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut current = String::new();
        for clip in clips {
            let label = day_label(clip.modified);
            if label != current {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&label).strong().color(theme::ACCENT));
                current = label;
            }
            widgets::card(ui, &clip.name, |ui| {
                ui.colored_label(
                    theme::MUTED,
                    format!(
                        "{}  \u{2022}  {}",
                        time_label(clip.modified),
                        size_label(clip.bytes)
                    ),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button(if clip.is_image { "View" } else { "Play" })
                        .clicked()
                    {
                        open_path(&clip.path);
                    }
                    if !clip.is_image && ui.button("Trim").clicked() {
                        app.screen = Screen::Trim(clip.path.clone());
                    }
                    if ui.button("Reveal").clicked() {
                        reveal(&clip.path);
                    }
                    if ui.button("Delete").clicked() {
                        if delete(&clip.path) {
                            app.toast(format!("deleted {}", clip.name), theme::WARN);
                        } else {
                            app.toast(format!("couldn't delete {}", clip.name), theme::DANGER);
                        }
                        app.gallery.scanned = None;
                    }
                });
            });
        }
    });
}
