use crate::app::{ScApp, Screen, ToastKind};
use crate::screens::sheet;
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
    pub game: String,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ViewKind {
    #[default]
    Originals,
    Trimmed,
    Compressed,
    All,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum GroupBy {
    #[default]
    Game,
    Date,
    Folder,
    Bookmark,
}

#[derive(Default)]
pub struct Cache {
    pub opened: Option<PathBuf>,
    clips: Vec<Clip>,
    dirs: Vec<PathBuf>,
    scanned: Option<Instant>,
    search: String,
    kind: ViewKind,
    group: GroupBy,
    only: Option<String>,
    folders: Vec<PathBuf>,
}

impl Cache {
    pub fn originals_count(&self, lib: &crate::library::Library) -> usize {
        self.clips
            .iter()
            .filter(|c| !lib.is_trimmed(&c.path) && !lib.is_compressed(&c.path))
            .count()
    }

    pub fn rescan(&mut self) {
        self.scanned = None;
    }

    pub fn reset_view(&mut self) {
        self.kind = ViewKind::Originals;
        self.group = GroupBy::Game;
        self.only = None;
        self.search.clear();
    }

    pub fn show_trimmed(&mut self) {
        self.kind = ViewKind::Trimmed;
        self.only = None;
    }

    pub fn show_compressed(&mut self) {
        self.kind = ViewKind::Compressed;
        self.only = None;
    }

    pub fn find(&self, path: &Path) -> Option<Clip> {
        self.clips.iter().find(|c| c.path == path).cloned()
    }

    pub fn folder_choices(&self, root: &Path) -> Vec<PathBuf> {
        let mut v = vec![root.to_path_buf()];
        v.extend(self.folders.iter().cloned());
        v
    }

    pub fn all_paths(&self) -> Vec<PathBuf> {
        self.clips.iter().map(|c| c.path.clone()).collect()
    }

    pub fn show_by(&mut self, folder: bool) {
        self.group = if folder {
            GroupBy::Folder
        } else {
            GroupBy::Bookmark
        };
    }

    pub fn first_video(&self) -> Option<PathBuf> {
        self.clips
            .iter()
            .find(|c| !c.is_image)
            .map(|c| c.path.clone())
    }

    fn refresh(&mut self, dirs: &[PathBuf], lib: &crate::library::Library) {
        let stale = self.dirs != dirs || self.scanned.is_none_or(|t| t.elapsed() > RESCAN_EVERY);
        if stale {
            let mut clips: Vec<Clip> = dirs.iter().flat_map(|d| scan(d)).collect();
            clips.sort_by_key(|c| std::cmp::Reverse(c.modified));
            for c in &mut clips {
                if let Some(g) = lib.game(&c.path) {
                    c.game = g.to_string();
                }
            }
            self.clips = clips;
            self.folders = dirs.first().map(|d| list_folders(d)).unwrap_or_default();
            self.dirs = dirs.to_vec();
            self.scanned = Some(Instant::now());
        }
    }
}

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

pub fn video_files(app: &mut ScApp) -> Vec<(PathBuf, u64)> {
    let dirs = library_dirs(app);
    app.gallery.refresh(&dirs, &app.library);
    app.gallery
        .clips
        .iter()
        .filter(|c| !c.is_image)
        .map(|c| (c.path.clone(), c.bytes))
        .collect()
}

pub fn recent_clips(app: &mut ScApp, n: usize) -> Vec<Clip> {
    let dirs = library_dirs(app);
    app.gallery.refresh(&dirs, &app.library);
    app.gallery
        .clips
        .iter()
        .filter(|c| !app.library.is_trimmed(&c.path) && !app.library.is_compressed(&c.path))
        .take(n)
        .cloned()
        .collect()
}

fn game_of(path: &Path, _root: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if !(stem == "sc" || stem.starts_with("sc-")) {
        return "Other".to_string();
    }
    let mut parts: Vec<&str> = stem.split('-').collect();
    if parts.first() == Some(&"sc") {
        parts.remove(0);
    }
    while let Some(last) = parts.last() {
        let digits = !last.is_empty() && last.chars().all(|c| c.is_ascii_digit());
        let size = last
            .strip_suffix("mb")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if digits || size || last.starts_with("trim") {
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

fn local(t: SystemTime) -> time::OffsetDateTime {
    let utc = time::OffsetDateTime::from(t);
    time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc)
}

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

pub fn play(app: &mut ScApp, path: &Path) {
    app.gallery.opened = Some(path.to_path_buf());
    open_path(path);
}

pub fn open_path(path: &Path) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

pub fn copy_file(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let name = path.to_string_lossy().to_string();
        let _clip = clipboard_win::Clipboard::new_attempts(10)
            .map_err(|_| "the clipboard is busy, try again".to_string())?;
        clipboard_win::raw::set_file_list_with(&[name], clipboard_win::options::DoClear)
            .map_err(|e| format!("couldn’t copy the file: {e}"))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("copying files is only available on Windows".to_string())
    }
}

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

pub fn delete(path: &Path) -> bool {
    #[cfg(windows)]
    if trash::delete(path).is_ok() {
        return true;
    }
    std::fs::remove_file(path).is_ok()
}

struct Group {
    label: String,
    color: Option<[u8; 3]>,
    folder: Option<PathBuf>,
    items: Vec<Clip>,
}

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
    app.gallery.refresh(&dirs, &app.library);

    let compressed_count = app
        .gallery
        .clips
        .iter()
        .filter(|c| app.library.is_compressed(&c.path))
        .count();
    let trimmed_count = app
        .gallery
        .clips
        .iter()
        .filter(|c| app.library.is_trimmed(&c.path) && !app.library.is_compressed(&c.path))
        .count();
    let originals_count = app.gallery.clips.len() - trimmed_count - compressed_count;
    let kind = app.gallery.kind;
    let in_kind: Vec<Clip> = app
        .gallery
        .clips
        .iter()
        .filter(|c| match kind {
            ViewKind::All => true,
            ViewKind::Trimmed => {
                app.library.is_trimmed(&c.path) && !app.library.is_compressed(&c.path)
            }
            ViewKind::Compressed => app.library.is_compressed(&c.path),
            ViewKind::Originals => {
                !app.library.is_trimmed(&c.path) && !app.library.is_compressed(&c.path)
            }
        })
        .cloned()
        .collect();

    let total: u64 = in_kind.iter().map(|c| c.bytes).sum();
    let count = in_kind.len();
    widgets::page_title(
        ui,
        "Clips",
        Some(&if count == 0 {
            "Nothing here yet".to_string()
        } else {
            format!(
                "{count} {} \u{2022} {}",
                if count == 1 { "item" } else { "items" },
                size_label(total)
            )
        }),
    );

    ui.horizontal(|ui| {
        widgets::segmented(
            ui,
            &mut app.gallery.kind,
            &[
                (ViewKind::Originals, &format!("Clips  {originals_count}")),
                (ViewKind::Trimmed, &format!("Edited  {trimmed_count}")),
                (
                    ViewKind::Compressed,
                    &format!("Compressed  {compressed_count}"),
                ),
                (ViewKind::All, "All"),
            ],
        );
    });
    ui.add_space(10.0);

    if app.gallery.clips.is_empty() && app.gallery.folders.is_empty() {
        widgets::card(ui, |ui| {
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                if theme::has_icons(ui.ctx()) {
                    ui.label(
                        RichText::new(icon::CLIPS)
                            .font(FontId::new(30.0, theme::icons()))
                            .color(p.text_3),
                    );
                    ui.add_space(6.0);
                }
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
                        widgets::pretty_combo(&app.cfg.hotkeys.save)
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

    if in_kind.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            widgets::secondary(
                ui,
                if kind == ViewKind::Trimmed {
                    "No edited clips yet. Open a clip and choose Edit."
                } else if kind == ViewKind::Compressed {
                    "No compressed clips yet. Open a clip and choose Compress for Discord."
                } else {
                    "Nothing here yet."
                },
            )
        });
        return;
    }
    toolbar(app, ui, &dir, &in_kind);
    ui.add_space(8.0);

    let by = app.gallery.group;
    let needle = app.gallery.search.trim().to_lowercase();
    let only = app.gallery.only.clone();
    let visible: Vec<Clip> = in_kind
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
    let empty_folders: Vec<PathBuf> = app
        .gallery
        .folders
        .iter()
        .filter(|f| {
            !app.gallery
                .clips
                .iter()
                .any(|c| c.path.parent() == Some(f.as_path()))
        })
        .cloned()
        .collect();
    let groups = build_groups(&visible, by, &dir, &app.library, &empty_folders, show_empty);
    if groups.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| widgets::secondary(ui, "Nothing matches."));
        return;
    }
    render_groups(app, ui, groups, by);
}

fn toolbar(app: &mut ScApp, ui: &mut Ui, dir: &Path, clips: &[Clip]) {
    let p = pal_ui(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
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

    let by = app.gallery.group;
    if by == GroupBy::Date {
        return;
    }
    let all = build_groups(clips, by, dir, &app.library, &app.gallery.folders, false);
    if all.len() > 1 {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let none = app.gallery.only.is_none();
            if chip(ui, &format!("All  {}", clips.len()), none, None) {
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

fn chip(ui: &mut Ui, label: &str, on: bool, dot: Option<[u8; 3]>) -> bool {
    let p = pal_ui(ui);
    let font = FontId::new(13.0, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let lead = if dot.is_some() { 16.0 } else { 0.0 };
    let size = galley.size() + vec2(24.0 + lead, 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let (bg, fg) = if on {
        (p.accent, theme::on_accent(&p))
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
                    .font(FontId::new(20.0, theme::semibold()))
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
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Empty. Move clips here from a clip’s details.")
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
                                app.toast(format!("Couldn’t remove it: {e}"), ToastKind::Error)
                            }
                        }
                    }
                }
            });
            continue;
        }

        let gap = 14.0;
        let avail = ui.available_width();
        let cols = (((avail + gap) / (240.0 + gap)).floor() as usize).max(1);
        let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        for row in g.items.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for clip in row {
                    card(app, ui, clip, w, by);
                }
            });
            ui.add_space(16.0);
        }
    }
}

pub fn preview(app: &mut ScApp, ui: &mut Ui, clip: &Clip, size: egui::Vec2) -> egui::Response {
    let p = pal_ui(ui);
    let rounding = Rounding::same(10.0);
    let slot = egui::Rect::from_min_size(ui.cursor().min, size);
    if !ui.is_rect_visible(slot) {
        let (_, resp) = ui.allocate_exact_size(size, Sense::click());
        return resp;
    }
    let resp = match app.thumbs.get(&clip.path, clip.is_image) {
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
    };
    if app.gallery.opened.as_deref() == Some(clip.path.as_path()) {
        let ring = resp.rect.expand(3.0);
        ui.painter().with_clip_rect(ring.expand(4.0)).rect_stroke(
            ring,
            Rounding::same(12.5),
            egui::Stroke::new(2.0_f32, p.accent.linear_multiply(0.7)),
        );
    }
    resp
}

pub fn recent_title(c: &Clip) -> String {
    if is_default_name(&c.path) {
        c.game.clone()
    } else {
        c.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&c.name)
            .to_string()
    }
}

fn is_default_name(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == "sc" || s.starts_with("sc-"))
}

fn clip_menu(app: &mut ScApp, ui: &mut Ui, clip: &Clip) {
    ui.set_min_width(190.0);
    let item = |ui: &mut Ui, label: &str| ui.add(egui::Button::new(label).frame(false)).clicked();

    if item(ui, "Play") {
        play(app, &clip.path);
        ui.close_menu();
    }
    if !clip.is_image && item(ui, "Edit") {
        app.goto(Screen::Edit(clip.path.clone()));
        ui.close_menu();
    }
    ui.separator();
    if item(ui, "Copy file") {
        match copy_file(&clip.path) {
            Ok(()) => app.toast(
                "Copied. Paste it in Discord or any chat with Ctrl+V",
                ToastKind::Ok,
            ),
            Err(e) => app.toast(e, ToastKind::Warn),
        }
        ui.close_menu();
    }
    if !clip.is_image {
        ui.menu_button("Compress for Discord", |ui| {
            for mb in [8u32, 10, 20, 25, 50, 100] {
                if item(ui, &format!("Fit {mb} MB")) {
                    app.share = Some(super::share::ShareState::compressing(
                        clip.path.clone(),
                        sc_core::compress::CompressOptions::with_size(mb),
                    ));
                    ui.close_menu();
                }
            }
            ui.separator();
            if item(ui, "More options\u{2026}") {
                app.share = Some(super::share::ShareState::choosing(clip.path.clone()));
                ui.close_menu();
            }
        });
    }
    if item(ui, "Show in folder") {
        reveal(&clip.path);
        ui.close_menu();
    }
    ui.separator();
    if item(ui, "Rename, notes and details\u{2026}") {
        app.sheet.open_clip(clip.path.clone());
        ui.close_menu();
    }
    ui.menu_button("Bookmarks", |ui| {
        let all = app.library.bookmarks.clone();
        let mine: Vec<u32> = app
            .library
            .bookmarks_of(&clip.path)
            .iter()
            .map(|b| b.id)
            .collect();
        for b in &all {
            let mark = if mine.contains(&b.id) {
                "\u{2713}  "
            } else {
                "     "
            };
            if item(ui, &format!("{mark}{}", b.name)) {
                app.library.toggle_bookmark(&clip.path, b.id);
            }
        }
        if !all.is_empty() {
            ui.separator();
        }
        if item(
            ui,
            if all.is_empty() {
                "Create a bookmark\u{2026}"
            } else {
                "Edit bookmarks\u{2026}"
            },
        ) {
            app.sheet.manage = true;
            ui.close_menu();
        }
    });
    ui.menu_button("Move to folder", |ui| {
        let root = app.cfg.save.resolve_dir();
        let here = clip
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for f in app.gallery.folder_choices(&root) {
            let label = folder_label(&f, &root);
            let current = f == here;
            let text = if current {
                format!("\u{2713}  {label}")
            } else {
                format!("     {label}")
            };
            if item(ui, &text) && !current {
                sheet::move_clip_to(app, clip, &f);
                ui.close_menu();
            }
        }
        ui.separator();
        if item(ui, "New folder\u{2026}") {
            sheet::move_clip_to_new_folder(app, clip);
            ui.close_menu();
        }
    });
    ui.separator();
    if item(ui, "Delete") {
        sheet::delete_clip(app, clip);
        ui.close_menu();
    }
}

fn card(app: &mut ScApp, ui: &mut Ui, clip: &Clip, w: f32, by: GroupBy) {
    let p = pal_ui(ui);
    let h = (w * 9.0 / 16.0).round();
    ui.vertical(|ui| {
        ui.set_width(w);
        ui.spacing_mut().item_spacing.y = 5.0;
        let resp = preview(app, ui, clip, vec2(w, h));
        let rect = resp.rect;
        resp.context_menu(|ui| clip_menu(app, ui, clip));
        let hovered = ui.rect_contains_pointer(rect);
        if resp.clicked() {
            play(app, &clip.path);
        }

        let badge = if app.library.is_compressed(&clip.path) {
            Some("Compressed")
        } else if app.library.is_trimmed(&clip.path) {
            Some("Trimmed")
        } else {
            None
        };
        if let Some(badge) = badge {
            let galley = ui.painter().layout_no_wrap(
                badge.to_string(),
                FontId::new(11.0, theme::semibold()),
                Color32::WHITE,
            );
            let tag = Rect::from_min_size(
                rect.left_top() + vec2(8.0, 8.0),
                galley.size() + vec2(14.0, 6.0),
            );
            ui.painter().rect_filled(
                tag,
                Rounding::same(tag.height() / 2.0),
                Color32::from_black_alpha(150),
            );
            ui.painter()
                .galley(tag.center() - galley.size() / 2.0, galley, Color32::WHITE);
        }

        if hovered {
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
                            app.toast("Couldn’t delete that file", ToastKind::Error);
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
                        && widgets::icon_button(ui, icon::CUT, "Edit", "Edit", false).clicked()
                    {
                        app.goto(Screen::Edit(clip.path.clone()));
                    }
                });
            });
            ui.ctx().request_repaint();
        }

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
        title.context_menu(|ui| clip_menu(app, ui, clip));
        if title
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.sheet.open_clip(clip.path.clone());
        }
        ui.add(
            egui::Label::new(RichText::new(line2).size(12.0).color(p.text_2))
                .truncate()
                .sense(Sense::click()),
        )
        .context_menu(|ui| clip_menu(app, ui, clip));

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

    #[cfg(windows)]
    #[test]
    #[ignore = "uses the real clipboard"]
    fn copied_file_lands_on_the_clipboard() {
        let file = std::env::temp_dir().join("sc-clipboard-test.mp4");
        std::fs::write(&file, b"x").unwrap();
        copy_file(&file).expect("copy");
        let _clip = clipboard_win::Clipboard::new_attempts(10).unwrap();
        let mut got = Vec::new();
        clipboard_win::raw::get_file_list_path(&mut got).expect("read back");
        assert_eq!(got, vec![file.clone()]);
        let _ = std::fs::remove_file(file);
    }

    fn g(p: &str) -> String {
        game_of(Path::new(p), Path::new("V"))
    }

    #[test]
    fn compressed_clips_group_with_their_game() {
        assert_eq!(g("V/sc-rainbowsix-20261019-171200-10mb.mp4"), "Rainbowsix");
        assert_eq!(g("V/sc-apex-20261017-210500-trim-25mb-2.mp4"), "Apex");
        assert_eq!(g("V/sc-20261019-181903-10mb.mp4"), "Desktop");
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
    fn game_comes_from_the_name_not_the_folder() {
        assert_eq!(g("V/Best of/sc-valorant-20261019.mp4"), "Valorant");
        assert_eq!(g("V/Best of/Triple kill.mp4"), "Other");
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
