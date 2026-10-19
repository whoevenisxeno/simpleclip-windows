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
    Folder,
    Bookmark,
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
    /// Show only this category (a game, folder or bookmark name), if set.
    only: Option<String>,
    /// Every sub-folder of the clips folder, including empty ones.
    folders: Vec<PathBuf>,
}

impl Cache {
    /// Force a rescan on the next frame.
    pub fn rescan(&mut self) {
        self.scanned = None;
    }

    /// The clip at `path`, if it is in the current listing.
    pub fn find(&self, path: &Path) -> Option<Clip> {
        self.clips.iter().find(|c| c.path == path).cloned()
    }

    /// Folders a clip can be moved to: the main folder first, then each sub-folder.
    pub fn folder_choices(&self, root: &Path) -> Vec<PathBuf> {
        let mut v = vec![root.to_path_buf()];
        v.extend(self.folders.iter().cloned());
        v
    }

    /// Paths of everything listed, newest first.
    pub fn all_paths(&self) -> Vec<PathBuf> {
        self.clips.iter().map(|c| c.path.clone()).collect()
    }

    /// Switch the view (used by the design-review harness).
    pub fn show_by(&mut self, folder: bool) {
        self.group = if folder {
            GroupBy::Folder
        } else {
            GroupBy::Bookmark
        };
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
            self.folders = dirs.first().map(|d| list_folders(d)).unwrap_or_default();
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

/// All sub-folders under `root` (depth-limited), sorted, including empty ones.
fn list_folders(root: &Path) -> Vec<PathBuf> {
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
                out.push(path.clone());
                go(&path, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    go(root, 0, &mut out);
    out.sort();
    out
}

/// "Main folder" for the clips folder itself, else the path inside it ("Best of / Aces").
pub fn folder_label(dir: &Path, root: &Path) -> String {
    if dir == root {
        return "Main folder".to_string();
    }
    match dir.strip_prefix(root) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(" / "),
        Err(_) => dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.display().to_string()),
    }
}

// -------------------------------------------------------------- formatting --

fn local(t: SystemTime) -> time::OffsetDateTime {
    let utc = time::OffsetDateTime::from(t);
    time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc)
}

/// "Today", "Yesterday", or "Mon 19 Oct" in the user's local time zone.
pub fn day_label(t: SystemTime) -> String {
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

pub fn time_label(t: SystemTime) -> String {
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

pub fn size_label(bytes: u64) -> String {
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
pub fn reveal(path: &Path) {
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
pub fn delete(path: &Path) -> bool {
    #[cfg(windows)]
    if trash::delete(path).is_ok() {
        return true;
    }
    std::fs::remove_file(path).is_ok()
}

// -------------------------------------------------------------------- view --

/// One labelled section of the gallery.
struct Group {
    label: String,
    color: Option<[u8; 3]>,
    /// For a folder group: the folder on disk (so an empty one can be removed).
    folder: Option<PathBuf>,
    items: Vec<Clip>,
}

/// The categories a clip belongs to under a grouping (a clip can be in several
/// bookmark groups at once).
fn keys_of(
    c: &Clip,
    by: GroupBy,
    root: &Path,
    lib: &crate::library::Library,
) -> Vec<(String, Option<[u8; 3]>)> {
    match by {
        GroupBy::Game => vec![(c.game.clone(), None)],
        GroupBy::Date => vec![(day_label(c.modified), None)],
        GroupBy::Folder => {
            let dir = c.path.parent().unwrap_or(root);
            vec![(folder_label(dir, root), None)]
        }
        GroupBy::Bookmark => {
            let marks = lib.bookmarks_of(&c.path);
            if marks.is_empty() {
                vec![("No bookmark".to_string(), None)]
            } else {
                marks
                    .iter()
                    .map(|b| (b.name.clone(), Some(b.color)))
                    .collect()
            }
        }
    }
}

/// Split clips into groups. `clips` arrive newest-first, so first-seen order is
/// already by recency. Folders are listed alphabetically (Main folder first) and
/// bookmarks in your own order; empty folders appear when `with_empty` is set.
fn build_groups(
    clips: &[Clip],
    by: GroupBy,
    root: &Path,
    lib: &crate::library::Library,
    folders: &[PathBuf],
    with_empty: bool,
) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for c in clips {
        for (label, color) in keys_of(c, by, root, lib) {
            match groups.iter_mut().find(|g| g.label == label) {
                Some(g) => g.items.push(c.clone()),
                None => groups.push(Group {
                    label,
                    color,
                    folder: None,
                    items: vec![c.clone()],
                }),
            }
        }
    }
    match by {
        GroupBy::Folder => {
            for f in folders {
                let label = folder_label(f, root);
                match groups.iter_mut().find(|g| g.label == label) {
                    Some(g) => g.folder = Some(f.clone()),
                    None if with_empty => groups.push(Group {
                        label,
                        color: None,
                        folder: Some(f.clone()),
                        items: Vec::new(),
                    }),
                    None => {}
                }
            }
            groups.sort_by(|a, b| {
                (a.label != "Main folder", a.label.to_lowercase())
                    .cmp(&(b.label != "Main folder", b.label.to_lowercase()))
            });
        }
        GroupBy::Bookmark => {
            groups.sort_by_key(|g| {
                lib.bookmarks
                    .iter()
                    .position(|b| b.name == g.label)
                    .unwrap_or(usize::MAX)
            });
        }
        _ => {}
    }
    groups
}

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

    if count == 0 && app.gallery.folders.is_empty() {
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

    let by = app.gallery.group;
    let needle = app.gallery.search.trim().to_lowercase();
    let only = app.gallery.only.clone();
    let visible: Vec<Clip> = app
        .gallery
        .clips
        .iter()
        .filter(|c| {
            let in_category = only.as_ref().is_none_or(|o| {
                keys_of(c, by, &dir, &app.library)
                    .iter()
                    .any(|(k, _)| k == o)
            });
            let matches_search = needle.is_empty()
                || c.name.to_lowercase().contains(&needle)
                || c.game.to_lowercase().contains(&needle)
                || app.library.note(&c.path).to_lowercase().contains(&needle)
                || app
                    .library
                    .bookmarks_of(&c.path)
                    .iter()
                    .any(|b| b.name.to_lowercase().contains(&needle));
            in_category && matches_search
        })
        .cloned()
        .collect();

    let show_empty = needle.is_empty() && only.is_none();
    let groups = build_groups(
        &visible,
        by,
        &dir,
        &app.library,
        &app.gallery.folders.clone(),
        show_empty,
    );
    if groups.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| widgets::secondary(ui, "Nothing matches."));
        return;
    }
    render_groups(app, ui, groups, by);
}

/// Search, grouping, actions, and the category chips.
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
                            .desired_width(120.0),
                    );
                });
            });
        let changed = widgets::segmented(
            ui,
            &mut app.gallery.group,
            &[
                (GroupBy::Game, "Game"),
                (GroupBy::Date, "Date"),
                (GroupBy::Folder, "Folder"),
                (GroupBy::Bookmark, "Bookmark"),
            ],
        );
        if changed {
            app.gallery.only = None;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::icon_button(ui, icon::FOLDER, "Open", "Open clips folder", false).clicked()
            {
                let _ = std::fs::create_dir_all(dir);
                open_path(dir);
            }
            if widgets::icon_button(ui, icon::REFRESH, "Refresh", "Refresh", false).clicked() {
                app.gallery.scanned = None;
            }
            if widgets::icon_button(ui, icon::TAG, "Tags", "Manage bookmarks", false).clicked() {
                app.sheet.manage = true;
            }
            if widgets::icon_button(ui, icon::ADD, "Folder", "New folder", false).clicked() {
                app.sheet.folder_dialog = Some(crate::screens::sheet::FolderDialog {
                    name: String::new(),
                    move_clip: None,
                });
            }
        });
    });

    // One chip per category of the current grouping, most clips first.
    let by = app.gallery.group;
    if by == GroupBy::Date {
        return;
    }
    let all = build_groups(
        &app.gallery.clips,
        by,
        dir,
        &app.library,
        &app.gallery.folders,
        false,
    );
    if all.len() > 1 {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let none = app.gallery.only.is_none();
            if chip(ui, &format!("All  {}", app.gallery.clips.len()), none, None) {
                app.gallery.only = None;
            }
            let mut sorted: Vec<&Group> = all.iter().collect();
            if by == GroupBy::Game {
                sorted.sort_by_key(|g| std::cmp::Reverse(g.items.len()));
            }
            for g in sorted {
                let on = app.gallery.only.as_deref() == Some(g.label.as_str());
                if chip(ui, &format!("{}  {}", g.label, g.items.len()), on, g.color) {
                    app.gallery.only = if on { None } else { Some(g.label.clone()) };
                }
            }
        });
    }
}

/// A category chip; bookmark chips carry their colour as a dot.
fn chip(ui: &mut Ui, label: &str, on: bool, dot: Option<[u8; 3]>) -> bool {
    let p = pal_ui(ui);
    let font = FontId::new(13.0, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let lead = if dot.is_some() { 16.0 } else { 0.0 };
    let size = galley.size() + vec2(24.0 + lead, 12.0);
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
    if let Some(c) = dot {
        ui.painter().circle_filled(
            pos2(rect.left() + 14.0, rect.center().y),
            4.5,
            Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    ui.painter().galley_with_override_text_color(
        pos2(
            rect.left() + 12.0 + lead,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn render_groups(app: &mut ScApp, ui: &mut Ui, groups: Vec<Group>, by: GroupBy) {
    let p = pal_ui(ui);
    for g in groups {
        let total: u64 = g.items.iter().map(|c| c.bytes).sum();
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if let Some(c) = g.color {
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 6.0, Color32::from_rgb(c[0], c[1], c[2]));
            }
            ui.label(
                RichText::new(&g.label)
                    .font(FontId::new(19.0, theme::semibold()))
                    .color(p.text),
            );
            if !g.items.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} \u{2022} {}",
                        if g.items.len() == 1 {
                            "1 item".to_string()
                        } else {
                            format!("{} items", g.items.len())
                        },
                        size_label(total)
                    ))
                    .size(13.0)
                    .color(p.text_2),
                );
            }
        });
        ui.add_space(6.0);

        if g.items.is_empty() {
            // An empty folder: say so, and let it be removed.
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Empty. Move clips here from a clip's details.")
                        .size(13.0)
                        .color(p.text_2),
                );
                if let Some(f) = &g.folder {
                    if widgets::quiet_button(ui, None, "Remove folder").clicked() {
                        match std::fs::remove_dir(f) {
                            Ok(()) => {
                                app.gallery.rescan();
                                app.toast("Folder removed", ToastKind::Ok);
                            }
                            Err(e) => {
                                app.toast(format!("Couldn't remove it: {e}"), ToastKind::Error)
                            }
                        }
                    }
                }
            });
            continue;
        }

        // Responsive grid: as many ~240px cards as fit.
        let gap = 14.0;
        let avail = ui.available_width();
        let cols = (((avail + gap) / (240.0 + gap)).floor() as usize).max(1);
        let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap, 16.0);
            for clip in &g.items {
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

/// Clips named by the app ("sc-game-date") show their time as the title; clips
/// you've named yourself show that name instead.
fn is_default_name(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == "sc" || s.starts_with("sc-"))
}

/// A clip card: preview with hover actions, then a caption, bookmarks and note.
fn card(app: &mut ScApp, ui: &mut Ui, clip: &Clip, w: f32, by: GroupBy) {
    let p = pal_ui(ui);
    let h = (w * 9.0 / 16.0).round();
    ui.vertical(|ui| {
        ui.set_width(w);
        ui.spacing_mut().item_spacing.y = 5.0;
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
                    if widgets::icon_button(
                        ui,
                        icon::MORE,
                        "More",
                        "Details, rename, bookmarks, notes",
                        false,
                    )
                    .clicked()
                    {
                        app.sheet.open_clip(clip.path.clone());
                    }
                    if widgets::icon_button(ui, icon::DELETE, "Delete", "Move to Recycle Bin", true)
                        .clicked()
                    {
                        if delete(&clip.path) {
                            app.library.forget(&clip.path);
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

        // Caption: the clip's own name if you gave it one, else when it happened.
        let when = format!("{} {}", day_label(clip.modified), time_label(clip.modified));
        let stem = clip
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&clip.name)
            .to_string();
        let custom = !is_default_name(&clip.path);
        let size = size_label(clip.bytes);
        let (line1, line2) = match (custom, by) {
            (true, GroupBy::Game) => (stem, format!("{when}  \u{2022}  {size}")),
            (true, _) => (
                stem,
                format!("{}  \u{2022}  {when}  \u{2022}  {size}", clip.game),
            ),
            (false, GroupBy::Date) => (
                clip.game.clone(),
                format!("{}  \u{2022}  {size}", time_label(clip.modified)),
            ),
            (false, _) => (when, size),
        };
        let title = ui.add(
            egui::Label::new(
                RichText::new(line1)
                    .font(FontId::new(13.5, theme::semibold()))
                    .color(p.text),
            )
            .truncate()
            .sense(Sense::click()),
        );
        if title
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.sheet.open_clip(clip.path.clone());
        }
        ui.add(egui::Label::new(RichText::new(line2).size(12.0).color(p.text_2)).truncate());

        // Bookmarks (click one to browse by it) and the start of the note.
        let marks: Vec<(String, [u8; 3])> = app
            .library
            .bookmarks_of(&clip.path)
            .iter()
            .map(|b| (b.name.clone(), b.color))
            .collect();
        if !marks.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (name, color) in marks.iter().take(3) {
                    if widgets::bookmark_pill(ui, name, *color, true, true).clicked() {
                        app.gallery.group = GroupBy::Bookmark;
                        app.gallery.only = Some(name.clone());
                    }
                }
                if marks.len() > 3 {
                    ui.label(
                        RichText::new(format!("+{}", marks.len() - 3))
                            .size(11.5)
                            .color(p.text_2),
                    );
                }
            });
        }
        let note = app.library.note(&clip.path);
        if let Some(first) = note.lines().find(|l| !l.trim().is_empty()) {
            ui.add(
                egui::Label::new(
                    RichText::new(first.trim())
                        .size(12.0)
                        .italics()
                        .color(p.text_3),
                )
                .truncate(),
            );
        }
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

    #[test]
    fn folder_labels_are_relative() {
        let root = Path::new("V");
        assert_eq!(folder_label(root, root), "Main folder");
        assert_eq!(folder_label(&root.join("Best of"), root), "Best of");
        assert_eq!(
            folder_label(&root.join("Best of").join("Aces"), root),
            "Best of / Aces"
        );
    }

    #[test]
    fn default_names_are_recognised() {
        assert!(is_default_name(Path::new("V/sc-apex-20261019.mp4")));
        assert!(!is_default_name(Path::new("V/my insane ace.mp4")));
    }
}
