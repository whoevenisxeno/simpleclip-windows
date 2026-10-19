use crate::app::{ScApp, Screen};
use crate::theme;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
struct Clip {
    path: PathBuf,
    name: String,
    modified: SystemTime,
    bytes: u64,
    is_image: bool,
    /// Game / app the clip came from ("Desktop" when unknown).
    game: String,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum GroupBy {
    #[default]
    Game,
    Date,
}

/// Cached directory scan, so the gallery doesn't re-walk the disk on every
/// repaint (the status poller repaints once a second), plus the view's filters.
#[derive(Default)]
pub struct Cache {
    clips: Vec<Clip>,
    dir: PathBuf,
    scanned: Option<Instant>,
    search: String,
    group: GroupBy,
    only_game: Option<String>,
}

impl Cache {
    /// Force a rescan on the next frame.
    pub fn rescan(&mut self) {
        self.scanned = None;
    }
}

const RESCAN_EVERY: Duration = Duration::from_secs(3);
const CARD_W: f32 = 248.0;
const THUMB_H: f32 = 140.0;

fn save_dir(app: &ScApp) -> PathBuf {
    app.cfg.save.resolve_dir()
}

/// `2026-10-19`-style day folder names, which say nothing about the game.
fn is_day_folder(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Work out which game a clip belongs to: the per-app folder it sits in, else
/// the `{app}` part of the default `sc-{app}-{date}` file name.
fn game_of(path: &Path, root: &Path) -> String {
    if let Some(folder) = path
        .parent()
        .filter(|p| *p != root)
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
    {
        if !is_day_folder(folder) {
            return pretty(folder);
        }
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let mut parts: Vec<&str> = stem.split('-').collect();
    if parts.first() == Some(&"sc") {
        parts.remove(0);
    }
    // Trailing date / time / "trim" markers aren't part of the name.
    while let Some(last) = parts.last() {
        let digits = !last.is_empty() && last.chars().all(|c| c.is_ascii_digit());
        if digits || last.starts_with("trim") {
            parts.pop();
        } else {
            break;
        }
    }
    let name = parts.join(" ");
    if name.is_empty() {
        "Desktop".to_string()
    } else {
        pretty(&name)
    }
}

fn pretty(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
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
            game: game_of(&path, dir),
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

fn local(t: SystemTime) -> time::OffsetDateTime {
    let utc = time::OffsetDateTime::from(t);
    time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc)
}

/// "Mon 19 Oct 2026" in the user's local time zone.
fn day_label(t: SystemTime) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let l = local(t);
    format!(
        "{} {} {} {}",
        DAYS[l.weekday().number_days_from_monday() as usize],
        l.day(),
        MONTHS[l.month() as usize - 1],
        l.year()
    )
}

fn time_label(t: SystemTime) -> String {
    let l = local(t);
    format!("{:02}:{:02}", l.hour(), l.minute())
}

fn size_label(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= 1024.0 * MB {
        format!("{:.1} GB", bytes as f64 / (1024.0 * MB))
    } else {
        format!("{:.1} MB", bytes as f64 / MB)
    }
}

pub fn open_path(path: &Path) {
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
    app.thumbs.poll();

    let stale = app.gallery.dir != dir
        || app
            .gallery
            .scanned
            .is_none_or(|t| t.elapsed() > RESCAN_EVERY);
    if stale {
        app.gallery.clips = scan(&dir);
        app.gallery.dir = dir.clone();
        app.gallery.scanned = Some(Instant::now());
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.heading("Gallery");
        ui.colored_label(theme::MUTED, format!("{} items", app.gallery.clips.len()));
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

    controls(app, ui);
    ui.add_space(6.0);

    // Apply the search box and game filter.
    let needle = app.gallery.search.trim().to_lowercase();
    let visible: Vec<Clip> = app
        .gallery
        .clips
        .iter()
        .filter(|c| {
            app.gallery.only_game.as_ref().is_none_or(|g| *g == c.game)
                && (needle.is_empty()
                    || c.name.to_lowercase().contains(&needle)
                    || c.game.to_lowercase().contains(&needle))
        })
        .cloned()
        .collect();
    if visible.is_empty() {
        ui.colored_label(theme::MUTED, "Nothing matches.");
        return;
    }
    render_groups(app, ui, visible);
}

/// Search box, group toggle and per-game filter chips.
fn controls(app: &mut ScApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.gallery.search)
                .hint_text("Search clips or games")
                .desired_width(220.0),
        );
        ui.add_space(8.0);
        ui.label("Group by:");
        ui.selectable_value(&mut app.gallery.group, GroupBy::Game, "Game");
        ui.selectable_value(&mut app.gallery.group, GroupBy::Date, "Date");
    });

    // One chip per game, most clips first.
    let mut counts: Vec<(String, usize)> = Vec::new();
    for c in &app.gallery.clips {
        match counts.iter_mut().find(|(g, _)| *g == c.game) {
            Some((_, n)) => *n += 1,
            None => counts.push((c.game.clone(), 1)),
        }
    }
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    if counts.len() > 1 {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            let all = app.gallery.only_game.is_none();
            if ui
                .selectable_label(all, format!("All ({})", app.gallery.clips.len()))
                .clicked()
            {
                app.gallery.only_game = None;
            }
            for (game, n) in counts {
                let on = app.gallery.only_game.as_deref() == Some(game.as_str());
                if ui.selectable_label(on, format!("{game} ({n})")).clicked() {
                    app.gallery.only_game = if on { None } else { Some(game) };
                }
            }
        });
    }
}

/// Clips split into labelled groups: newest group first, newest clip first.
fn grouped(clips: Vec<Clip>, by: GroupBy) -> Vec<(String, Vec<Clip>)> {
    let mut groups: Vec<(String, Vec<Clip>)> = Vec::new();
    for c in clips {
        let key = match by {
            GroupBy::Game => c.game.clone(),
            GroupBy::Date => day_label(c.modified),
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(c),
            None => groups.push((key, vec![c])),
        }
    }
    // `clips` arrive newest-first, so first-seen order is already by recency.
    groups
}

fn render_groups(app: &mut ScApp, ui: &mut egui::Ui, clips: Vec<Clip>) {
    let groups = grouped(clips, app.gallery.group);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (label, items) in groups {
            let total: u64 = items.iter().map(|c| c.bytes).sum();
            let title = egui::RichText::new(format!(
                "{label}   \u{2022}   {} \u{2022} {}",
                if items.len() == 1 {
                    "1 item".to_string()
                } else {
                    format!("{} items", items.len())
                },
                size_label(total)
            ))
            .strong()
            .color(theme::ACCENT);
            egui::CollapsingHeader::new(title)
                .id_salt((&label, app.gallery.group == GroupBy::Game))
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                        for clip in &items {
                            card(app, ui, clip);
                        }
                    });
                });
            ui.add_space(6.0);
        }
    });
}

fn card(app: &mut ScApp, ui: &mut egui::Ui, clip: &Clip) {
    egui::Frame::none()
        .fill(ui.visuals().faint_bg_color)
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ui.set_width(CARD_W);
            let size = egui::vec2(CARD_W, THUMB_H);
            let resp = match app.thumbs.get(&clip.path, clip.is_image) {
                Some(tex) => ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                        .fit_to_exact_size(size)
                        .rounding(egui::Rounding::same(6.0))
                        .sense(egui::Sense::click()),
                ),
                None => {
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                    ui.painter()
                        .rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        if clip.is_image {
                            "\u{1F5BC}"
                        } else {
                            "\u{25B6}"
                        },
                        egui::FontId::proportional(28.0),
                        theme::MUTED,
                    );
                    resp
                }
            };
            if resp.on_hover_text("Open").clicked() {
                open_path(&clip.path);
            }

            ui.add_space(4.0);
            ui.label(egui::RichText::new(&clip.name).size(12.0));
            ui.colored_label(
                theme::MUTED,
                format!(
                    "{} {}  \u{2022}  {}",
                    day_label(clip.modified),
                    time_label(clip.modified),
                    size_label(clip.bytes)
                ),
            );
            ui.horizontal(|ui| {
                if ui
                    .small_button(if clip.is_image { "View" } else { "Play" })
                    .clicked()
                {
                    open_path(&clip.path);
                }
                if !clip.is_image && ui.small_button("Trim").clicked() {
                    app.screen = Screen::Trim(clip.path.clone());
                }
                if ui.small_button("Reveal").clicked() {
                    reveal(&clip.path);
                }
                if ui.small_button("Delete").clicked() {
                    if delete(&clip.path) {
                        app.toast(format!("deleted {}", clip.name), theme::WARN);
                    } else {
                        app.toast(format!("couldn't delete {}", clip.name), theme::DANGER);
                    }
                    app.thumbs.forget(&clip.path);
                    app.gallery.scanned = None;
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(p: &str) -> String {
        game_of(Path::new(p), Path::new("V"))
    }

    #[test]
    fn game_comes_from_the_default_file_name() {
        assert_eq!(g("V/2026-10-19/sc-rainbowsix-20261019.mp4"), "Rainbowsix");
        assert_eq!(
            g("V/2026-10-19/sc-rainbowsix-20261019-154732.mp4"),
            "Rainbowsix"
        );
        assert_eq!(g("V/sc-apex-legends-20261019-trim.mp4"), "Apex legends");
    }

    #[test]
    fn no_app_means_desktop() {
        assert_eq!(g("V/2026-10-19/sc-20261019.mp4"), "Desktop");
    }

    #[test]
    fn per_app_folder_wins() {
        assert_eq!(g("V/valorant/sc-valorant-20261019.mp4"), "Valorant");
        assert_eq!(g("V/valorant/anything.mp4"), "Valorant");
    }

    #[test]
    fn day_folders_are_not_games() {
        assert!(is_day_folder("2026-10-19"));
        assert!(!is_day_folder("valorant"));
    }
}
