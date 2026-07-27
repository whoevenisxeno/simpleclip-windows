use crate::app::{ScApp, Screen};
use crate::{theme, widgets};
use std::path::PathBuf;
use std::time::SystemTime;

struct Clip {
    path: PathBuf,
    name: String,
    modified: SystemTime,
    is_image: bool,
}

fn save_dir(app: &ScApp) -> PathBuf {
    app.cfg
        .save
        .directory
        .clone()
        .or_else(|| directories::UserDirs::new().and_then(|d| d.video_dir().map(PathBuf::from)))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn scan(dir: &std::path::Path) -> Vec<Clip> {
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
        let modified = path
            .metadata()
            .and_then(|m| m.modified())
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
            is_image,
        });
    }
    clips.sort_by_key(|c| std::cmp::Reverse(c.modified));
    clips
}

/// Small recursive directory walk (max 3 levels) so per-day / per-app subfolders
/// are picked up without pulling in an extra crate.
fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    fn go(dir: &std::path::Path, depth: u8, out: &mut Vec<PathBuf>) {
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

fn day_label(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Group key only; a human date formatter arrives with the gallery polish pass.
    format!("day {days}")
}

fn open_path(path: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

pub fn view(app: &mut ScApp, ui: &mut egui::Ui) {
    let dir = save_dir(app);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.heading("Gallery");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Open folder").clicked() {
                open_path(&dir);
            }
        });
    });
    ui.colored_label(theme::MUTED, dir.display().to_string());
    ui.separator();

    let clips = scan(&dir);
    if clips.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.colored_label(
                theme::MUTED,
                "No clips yet. Hit your save hotkey during a session.",
            );
        });
        return;
    }
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
                        if let Some(parent) = clip.path.parent() {
                            open_path(parent);
                        }
                    }
                    if ui.button("Delete").clicked() {
                        let _ = std::fs::remove_file(&clip.path);
                        app.toast(format!("deleted {}", clip.name), theme::WARN);
                    }
                });
            });
        }
    });
}
