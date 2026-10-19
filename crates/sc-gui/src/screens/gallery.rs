use crate::app::{ScApp, Screen, ToastKind};
use crate::theme::{self, icon, mix, pal_ui};
use crate::widgets;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, RichText, Rounding, Sense, Ui,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
pub struct Clip {
    pub path: PathBuf,
    pub name: String,
    pub modified: SystemTime,
    pub bytes: u64,
    pub is_image: bool,
    /// Game / app the clip came from ("Desktop" when unknown).
    pub game: String,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum GroupBy {
    #[default]
    Game,
    Date,
}

/// Cached directory scan, so the gallery doesn't re-walk the disk on every
/// repaint, plus the view's filters.
#[derive(Default)]
pub struct Cache {
    clips: Vec<Clip>,
    dirs: Vec<PathBuf>,
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

    /// First video in the list (newest), for the design-review harness.
    pub fn first_video(&self) -> Option<PathBuf> {
        self.clips
            .iter()
            .find(|c| !c.is_image)
            .map(|c| c.path.clone())
    }

    fn refresh(&mut self, dirs: &[PathBuf]) {
        let stale = self.dirs != dirs || self.scanned.is_none_or(|t| t.elapsed() > RESCAN_EVERY);
        if stale {
            let mut clips: Vec<Clip> = dirs.iter().flat_map(|d| scan(d)).collect();
            clips.sort_by_key(|c| std::cmp::Reverse(c.modified));
            self.clips = clips;
            self.dirs = dirs.to_vec();
            self.scanned = Some(Instant::now());
        }
    }
}

/// Folders to list: the clips folder, plus the trim folder when it lives elsewhere.
fn library_dirs(app: &ScApp) -> Vec<PathBuf> {
    let main = app.cfg.save.resolve_dir();
    let mut dirs = vec![main.clone()];
    if let Some(t) = &app.cfg.save.trim_directory {
        if !t.starts_with(&main) && !main.starts_with(t) {
            dirs.push(t.clone());
        }
    }
    dirs
}

const RESCAN_EVERY: Duration = Duration::from_secs(3);

/// The newest `n` items, scanning first if the cache is stale.
pub fn recent_clips(app: &mut ScApp, n: usize) -> Vec<Clip> {
    let dirs = library_dirs(app);
    app.gallery.refresh(&dirs);
    app.gallery.clips.iter().take(n).cloned().collect()
}

// ---------------------------------------------------------------- scanning --

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
    for path in walkdir(dir) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(ext.as_str(), "mp4" | "mkv" | "png") {
            continue;
        }
        let meta = path.metadata().ok();
        clips.push(Clip {
            game: game_of(&path, dir),
            name: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("clip")
                .to_string(),
            modified: meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH),
            bytes: meta.map(|m| m.len()).unwrap_or(0),
            is_image: ext == "png",
            path,
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

// -------------------------------------------------------------- formatting --

fn local(t: SystemTime) -> time::OffsetDateTime {
    let utc = time::OffsetDateTime::from(t);
    time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc)
}

/// "Today", "Yesterday", or "Mon 19 Oct" in the user's local time zone.
fn day_label(t: SystemTime) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let l = local(t);
    let today = local(SystemTime::now());
    if l.date() == today.date() {
        return "Today".into();
    }
    if Some(l.date()) == today.date().previous_day() {
        return "Yesterday".into();
    }
    format!(
        "{} {} {}",
        DAYS[l.weekday().number_days_from_monday() as usize],
        l.day(),
        MONTHS[l.month() as usize - 1],
    )
}

fn time_label(t: SystemTime) -> String {
    let l = local(t);
    format!("{:02}:{:02}", l.hour(), l.minute())
}

/// "5 min ago", "Today 14:12", "Yesterday 21:03".
pub fn ago(t: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        _ => format!("{} {}", day_label(t), time_label(t)),
    }
}

fn size_label(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= 1024.0 * MB {
        format!("{:.1} GB", bytes as f64 / (1024.0 * MB))
    } else {
        format!("{:.0} MB", bytes as f64 / MB)
    }
}

// ------------------------------------------------------------ file actions --

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

// -------------------------------------------------------------------- view --

pub fn view(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let dir = app.cfg.save.resolve_dir();
    let dirs = library_dirs(app);
    app.gallery.refresh(&dirs);

    let total: u64 = app.gallery.clips.iter().map(|c| c.bytes).sum();
    let count = app.gallery.clips.len();
    widgets::page_title(
        ui,
        "Clips",
        Some(&if count == 0 {
            "Nothing saved yet".to_string()
        } else {
            format!(
                "{count} {} \u{2022} {}",
                if count == 1 { "item" } else { "items" },
                size_label(total)
            )
        }),
    );

    if count == 0 {
        widgets::card(ui, |ui| {
            ui.add_space(22.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No clips yet")
                        .font(FontId::new(20.0, theme::semibold()))
                        .color(p.text),
                );
                ui.add_space(4.0);
                widgets::secondary(
                    ui,
                    format!(
                        "Press {} during a session to keep the last moments.",
                        app.cfg.hotkeys.save
                    ),
                );
                ui.add_space(14.0);
                if widgets::quiet_button(ui, Some(icon::FOLDER), "Open folder").clicked() {
                    let _ = std::fs::create_dir_all(&dir);
                    open_path(&dir);
                }
            });
            ui.add_space(22.0);
        });
        return;
    }

    toolbar(app, ui, &dir);
    ui.add_space(8.0);

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
        ui.add_space(30.0);
        ui.vertical_centered(|ui| widgets::secondary(ui, "Nothing matches your search."));
        return;
    }
    groups(app, ui, visible);
}

/// Search, grouping, and the per-game filter chips.
fn toolbar(app: &mut ScApp, ui: &mut Ui, dir: &Path) {
    let p = pal_ui(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // Search field.
        egui::Frame::none()
            .fill(p.card_hi)
            .rounding(Rounding::same(9.0))
            .inner_margin(egui::Margin::symmetric(10.0, 0.0))
            .show(ui, |ui| {
                ui.set_height(30.0);
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if theme::has_icons(ui.ctx()) {
                        ui.label(
                            RichText::new(icon::SEARCH)
                                .family(theme::icons())
                                .size(13.0)
                                .color(p.text_2),
                        );
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut app.gallery.search)
                            .hint_text("Search")
                            .frame(false)
                            .desired_width(170.0),
                    );
                });
            });
        widgets::segmented(
            ui,
            &mut app.gallery.group,
            &[(GroupBy::Game, "By game"), (GroupBy::Date, "By date")],
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icon::FOLDER, "Open", "Open clips folder", false).clicked()
            {
                let _ = std::fs::create_dir_all(dir);
                open_path(dir);
            }
            if widgets::icon_button(ui, icon::REFRESH, "Refresh", "Refresh", false).clicked() {
                app.gallery.scanned = None;
            }
        });
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
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let all = app.gallery.only_game.is_none();
            if chip(ui, &format!("All  {}", app.gallery.clips.len()), all) {
                app.gallery.only_game = None;
            }
            for (game, n) in counts {
                let on = app.gallery.only_game.as_deref() == Some(game.as_str());
                if chip(ui, &format!("{game}  {n}"), on) {
                    app.gallery.only_game = if on { None } else { Some(game) };
                }
            }
        });
    }
}

fn chip(ui: &mut Ui, label: &str, on: bool) -> bool {
    let p = pal_ui(ui);
    let font = FontId::new(13.0, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let size = galley.size() + vec2(24.0, 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (bg, fg) = if on {
        (p.accent, Color32::WHITE)
    } else if resp.hovered() {
        (mix(p.card_hi, p.text, 0.06), p.text)
    } else {
        (p.card_hi, p.text)
    };
    ui.painter()
        .rect_filled(rect, Rounding::same(size.y / 2.0), bg);
    ui.painter()
        .galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, fg);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
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

fn groups(app: &mut ScApp, ui: &mut Ui, clips: Vec<Clip>) {
    let p = pal_ui(ui);
    let by = app.gallery.group;
    for (label, items) in grouped(clips, by) {
        let total: u64 = items.iter().map(|c| c.bytes).sum();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&label)
                    .font(FontId::new(19.0, theme::semibold()))
                    .color(p.text),
            );
            ui.label(
                RichText::new(format!(
                    "{} \u{2022} {}",
                    if items.len() == 1 {
                        "1 item".to_string()
                    } else {
                        format!("{} items", items.len())
                    },
                    size_label(total)
                ))
                .size(13.0)
                .color(p.text_2),
            );
        });
        ui.add_space(6.0);

        // Responsive grid: as many ~240px cards as fit.
        let gap = 14.0;
        let avail = ui.available_width();
        let cols = (((avail + gap) / (240.0 + gap)).floor() as usize).max(1);
        let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap, 16.0);
            for clip in &items {
                card(app, ui, clip, w, by);
            }
        });
    }
}

/// A rounded preview image (or a placeholder while it loads).
pub fn preview(app: &mut ScApp, ui: &mut Ui, clip: &Clip, size: egui::Vec2) -> egui::Response {
    let p = pal_ui(ui);
    let rounding = Rounding::same(10.0);
    match app.thumbs.get(&clip.path, clip.is_image) {
        Some(tex) => ui.add(
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                .fit_to_exact_size(size)
                .rounding(rounding)
                .sense(Sense::click()),
        ),
        None => {
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            ui.painter().rect_filled(rect, rounding, p.card_hi);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                if app.thumbs.gave_up(&clip.path) {
                    "Preview unavailable"
                } else if clip.is_image {
                    "Image"
                } else {
                    "Loading"
                },
                FontId::new(12.0, egui::FontFamily::Proportional),
                p.text_3,
            );
            resp
        }
    }
}

/// A clip card: preview with hover actions, then a caption.
fn card(app: &mut ScApp, ui: &mut Ui, clip: &Clip, w: f32, by: GroupBy) {
    let p = pal_ui(ui);
    let h = (w * 9.0 / 16.0).round();
    ui.vertical(|ui| {
        ui.set_width(w);
        ui.spacing_mut().item_spacing.y = 6.0;
        let resp = preview(app, ui, clip, vec2(w, h));
        let rect = resp.rect;
        let hovered = ui.rect_contains_pointer(rect);
        if resp.clicked() {
            open_path(&clip.path);
        }

        if hovered {
            // Soft scrim, a play glyph, and the quick actions along the bottom.
            ui.painter()
                .rect_filled(rect, Rounding::same(10.0), Color32::from_black_alpha(70));
            let c = rect.center();
            ui.painter()
                .circle_filled(c, 22.0, Color32::from_white_alpha(235));
            if theme::has_icons(ui.ctx()) {
                ui.painter().text(
                    pos2(c.x + 1.0, c.y),
                    Align2::CENTER_CENTER,
                    if clip.is_image {
                        icon::CLIPS
                    } else {
                        icon::PLAY
                    },
                    FontId::new(18.0, theme::icons()),
                    Color32::from_rgb(0x1d, 0x1d, 0x1f),
                );
            }
            let bar = Rect::from_min_max(
                pos2(rect.left() + 8.0, rect.bottom() - 40.0),
                pos2(rect.right() - 8.0, rect.bottom() - 6.0),
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if widgets::icon_button(ui, icon::DELETE, "Delete", "Move to Recycle Bin", true)
                        .clicked()
                    {
                        if delete(&clip.path) {
                            app.toast("Moved to the Recycle Bin", ToastKind::Ok);
                        } else {
                            app.toast("Couldn't delete that file", ToastKind::Error);
                        }
                        app.thumbs.forget(&clip.path);
                        app.gallery.rescan();
                    }
                    if widgets::icon_button(ui, icon::FOLDER, "Show", "Show in folder", false)
                        .clicked()
                    {
                        reveal(&clip.path);
                    }
                    if !clip.is_image
                        && widgets::icon_button(ui, icon::CUT, "Trim", "Trim", false).clicked()
                    {
                        app.goto(Screen::Trim(clip.path.clone()));
                    }
                });
            });
            ui.ctx().request_repaint();
        }

        // Caption.
        let (line1, line2) = match by {
            GroupBy::Game => (
                format!("{} {}", day_label(clip.modified), time_label(clip.modified)),
                size_label(clip.bytes),
            ),
            GroupBy::Date => (
                clip.game.clone(),
                format!(
                    "{}  \u{2022}  {}",
                    time_label(clip.modified),
                    size_label(clip.bytes)
                ),
            ),
        };
        ui.add(
            egui::Label::new(
                RichText::new(line1)
                    .font(FontId::new(13.5, theme::semibold()))
                    .color(p.text),
            )
            .truncate(),
        );
        ui.add(egui::Label::new(RichText::new(line2).size(12.0).color(p.text_2)).truncate());
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
