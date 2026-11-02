use crate::addon::{self, Home};
use crate::app::{ScApp, Screen, ToastKind};
use crate::library::{self, COLORS};
use crate::screens::gallery::{self, Clip};
use crate::theme::{self, icon, pal};
use crate::widgets;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Order, Rect, RichText, Rounding, Sense, Ui,
};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct SheetState {
    pub open: Option<PathBuf>,
    name_buf: String,
    note_buf: String,
    loaded_for: Option<PathBuf>,
    pub manage: bool,
    new_name: String,
    new_color: usize,
    pub folder_dialog: Option<FolderDialog>,
    error: Option<String>,
    pub ask: Option<AskDialog>,
    answers: HashMap<(String, String), Vec<String>>,
}

pub struct FolderDialog {
    pub name: String,
    pub move_clip: Option<PathBuf>,
}

pub struct AskDialog {
    pub home: Home,
    pub clip: Option<PathBuf>,
    pub values: Vec<String>,
    focused: bool,
}

impl AskDialog {
    pub fn new(home: Home, clip: Option<PathBuf>, mut values: Vec<String>) -> Self {
        values.resize(home.button.ask.len(), String::new());
        Self {
            home,
            clip,
            values,
            focused: false,
        }
    }

    pub fn ready(&self) -> bool {
        self.values.iter().any(|v| !v.trim().is_empty())
    }

    pub fn answers(&self) -> Vec<(String, String)> {
        self.home
            .button
            .ask
            .iter()
            .zip(&self.values)
            .map(|(a, v)| (a.key.clone(), v.trim().to_string()))
            .collect()
    }

    fn key(&self) -> (String, String) {
        (self.home.addon.clone(), self.home.button.id.clone())
    }
}

impl SheetState {
    pub fn open_clip(&mut self, path: PathBuf) {
        self.open = Some(path);
        self.loaded_for = None;
        self.error = None;
    }

    pub fn ask_for(&mut self, home: Home, clip: Option<PathBuf>) {
        let key = (home.addon.clone(), home.button.id.clone());
        let last = self.answers.get(&key).cloned().unwrap_or_default();
        self.ask = Some(AskDialog::new(home, clip, last));
    }

    fn remember(&mut self, d: &AskDialog) {
        self.answers.insert(d.key(), d.values.clone());
    }
}

pub(crate) fn start_addon(app: &mut ScApp, h: &Home, clip: Option<PathBuf>, ctx: &egui::Context) {
    if h.button.ask.is_empty() {
        let clips = crate::screens::gallery::library_dirs(app);
        app.settings.addon.press(h, clip, &[], clips, ctx);
    } else {
        app.sheet.ask_for(h.clone(), clip);
    }
}

pub(crate) fn busy_text(h: &Home) -> String {
    let b = h.button.busy.trim().trim_end_matches(['.', '\u{2026}']);
    if b.is_empty() {
        "Working\u{2026}".into()
    } else {
        format!("{b}\u{2026}")
    }
}

pub(crate) fn addon_row(
    ui: &mut Ui,
    list: &[Home],
    busy: bool,
    at: Option<usize>,
) -> Option<usize> {
    let mut hit = None;
    for (i, h) in list.iter().enumerate() {
        if at == Some(i) {
            let r = ui
                .add_enabled_ui(false, |ui| {
                    widgets::quiet_button(ui, Some(""), &busy_text(h))
                })
                .inner;
            let spot = Rect::from_center_size(
                pos2(r.rect.left() + 22.0, r.rect.center().y),
                vec2(14.0, 14.0),
            );
            egui::Spinner::new()
                .color(theme::pal_ui(ui).accent)
                .paint_at(ui, spot);
            continue;
        }
        let glyph = addon::glyph(&h.button.icon);
        let mut r = ui
            .add_enabled_ui(!busy, |ui| {
                widgets::quiet_button(ui, Some(glyph), &h.button.label)
            })
            .inner;
        if !h.button.hint.is_empty() {
            r = r.on_hover_text(&h.button.hint);
        }
        if r.clicked() {
            hit = Some(i);
        }
    }
    hit
}

pub(crate) fn ask_form(ui: &mut Ui, d: &mut AskDialog) -> Option<bool> {
    let mut out = None;
    let mut enter = false;
    ui.spacing_mut().item_spacing.y = 10.0;
    title(ui, &d.home.button.label);
    if !d.home.button.hint.is_empty() {
        widgets::secondary(ui, &d.home.button.hint);
    }
    let w = ui.available_width();
    for (i, a) in d.home.button.ask.iter().enumerate() {
        field_label(ui, if a.label.is_empty() { &a.key } else { &a.label });
        let max = if a.max == 0 {
            usize::MAX
        } else {
            a.max as usize
        };
        let r = limited_box(ui, &mut d.values[i], &a.hint, w, max);
        if i == 0 && !d.focused {
            r.request_focus();
            d.focused = true;
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            enter = true;
        }
    }
    let ready = d.ready();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if widgets::quiet_button(ui, None, "Cancel").clicked() {
            out = Some(false);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let glyph = addon::glyph(&d.home.button.icon);
            let r = ui
                .add_enabled_ui(ready, |ui| {
                    widgets::primary_button(ui, Some(glyph), &d.home.button.label)
                })
                .inner;
            if r.clicked() {
                out = Some(true);
            }
        });
    });
    if enter && ready {
        out = Some(true);
    }
    out
}

fn ask_dialog(app: &mut ScApp, ctx: &egui::Context) {
    let Some(mut d) = app.sheet.ask.take() else {
        return;
    };
    let mut got = None;
    let close = modal(ctx, "addon-ask", 420.0, |ui| got = ask_form(ui, &mut d));
    app.sheet.remember(&d);
    match got {
        Some(true) => {
            let clips = crate::screens::gallery::library_dirs(app);
            app.settings
                .addon
                .press(&d.home, d.clip.clone(), &d.answers(), clips, ctx);
        }
        Some(false) => {}
        None if close => {}
        None => app.sheet.ask = Some(d),
    }
}

pub fn show(app: &mut ScApp, ctx: &egui::Context) {
    if app.sheet.ask.is_some() {
        ask_dialog(app, ctx);
    } else if app.sheet.folder_dialog.is_some() {
        folder_dialog(app, ctx);
    } else if app.sheet.manage {
        manager(app, ctx);
    } else if app.sheet.open.is_some() {
        details(app, ctx);
    }
}

pub(crate) fn modal(ctx: &egui::Context, id: &str, width: f32, add: impl FnOnce(&mut Ui)) -> bool {
    let p = pal(ctx);
    let screen = ctx.screen_rect();
    let mut close = false;
    egui::Area::new(egui::Id::new((id, "scrim")))
        .order(Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(
                rect,
                0.0,
                Color32::from_black_alpha(if p.dark { 150 } else { 80 }),
            );
            if resp.clicked() {
                close = true;
            }
        });
    egui::Area::new(egui::Id::new(id))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(p.card)
                .rounding(Rounding::same(theme::radius::SHEET))
                .stroke(egui::Stroke::new(1.0_f32, p.line))
                .shadow(theme::float_shadow(&p))
                .inner_margin(egui::Margin::same(22.0))
                .show(ui, |ui| {
                    ui.set_width(width);
                    add(ui);
                });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    close
}

fn title(ui: &mut Ui, text: &str) {
    let p = theme::pal_ui(ui);
    ui.label(
        RichText::new(text)
            .font(FontId::new(22.0, theme::semibold()))
            .color(p.text),
    );
}

fn field_label(ui: &mut Ui, text: &str) {
    let p = theme::pal_ui(ui);
    ui.label(
        RichText::new(text)
            .font(FontId::new(12.5, theme::semibold()))
            .color(p.text_2),
    );
}

fn text_box(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> egui::Response {
    limited_box(ui, text, hint, width, usize::MAX)
}

fn limited_box(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    width: f32,
    max: usize,
) -> egui::Response {
    let p = theme::pal_ui(ui);
    egui::Frame::none()
        .fill(p.card_hi)
        .rounding(Rounding::same(9.0))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .char_limit(max)
                    .frame(false)
                    .desired_width(width - 20.0),
            )
        })
        .inner
}

fn details(app: &mut ScApp, ctx: &egui::Context) {
    let Some(path) = app.sheet.open.clone() else {
        return;
    };
    let Some(clip) = app.gallery.find(&path) else {
        app.sheet.open = None;
        return;
    };
    if app.sheet.loaded_for.as_ref() != Some(&path) {
        app.sheet.name_buf = clip
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        app.sheet.note_buf = app.library.note(&path).to_string();
        app.sheet.loaded_for = Some(path.clone());
    }

    let mut done = false;
    let close = modal(ctx, "clip-details", 720.0, |ui| {
        let p = theme::pal_ui(ui);
        ui.spacing_mut().item_spacing.y = 8.0;
        let left_w = 300.0;
        let right_w = 720.0 - left_w - 28.0;

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 28.0;

            ui.vertical(|ui| {
                ui.set_width(left_w);
                let resp =
                    gallery::preview(app, ui, &clip, vec2(left_w, (left_w * 9.0 / 16.0).round()));
                if resp.on_hover_text("Play").clicked() {
                    gallery::play(app, &clip.path);
                }
                ui.label(
                    RichText::new(format!(
                        "{}  \u{2022}  {}",
                        gallery::ago(clip.modified),
                        gallery::size_label(clip.bytes)
                    ))
                    .size(12.5)
                    .color(p.text_2),
                );
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                if widgets::quiet_button(ui, Some(icon::PLAY), "Play").clicked() {
                    gallery::play(app, &clip.path);
                }
                if !clip.is_image && widgets::quiet_button(ui, Some(icon::CUT), "Edit").clicked() {
                    app.goto(Screen::Edit(clip.path.clone()));
                    done = true;
                }
                if widgets::quiet_button(ui, Some(icon::COPY), "Copy file").clicked() {
                    match gallery::copy_file(&clip.path) {
                        Ok(()) => app.toast(
                            "Copied. Paste it in Discord or any chat with Ctrl+V",
                            ToastKind::Ok,
                        ),
                        Err(e) => app.toast(e, ToastKind::Warn),
                    }
                }
                if !clip.is_image
                    && widgets::quiet_button(ui, Some(icon::SHARE), "Compress for Discord\u{2026}")
                        .clicked()
                {
                    app.share = Some(super::share::ShareState::choosing(clip.path.clone()));
                    done = true;
                }
                if !clip.is_image {
                    clip_addons(app, ui, &clip);
                }
                if widgets::quiet_button(ui, Some(icon::FOLDER), "Show in folder").clicked() {
                    gallery::reveal(&clip.path);
                }
                if widgets::quiet_button(ui, Some(icon::DELETE), "Delete").clicked() {
                    delete_clip(app, &clip);
                    done = true;
                }
            });

            ui.vertical(|ui| {
                ui.set_width(right_w);
                title(ui, "Details");
                ui.add_space(2.0);

                field_label(ui, "Name");
                ui.horizontal(|ui| {
                    let ext = clip
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("mp4")
                        .to_string();
                    let ext_w = 44.0;
                    let r = text_box(ui, &mut app.sheet.name_buf, "Clip name", right_w - ext_w);
                    ui.label(RichText::new(format!(".{ext}")).color(p.text_3));
                    if r.lost_focus() {
                        rename(app, &clip);
                    }
                });

                ui.add_space(2.0);
                field_label(ui, "Bookmarks");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    let all = app.library.bookmarks.clone();
                    let mine: Vec<u32> = app
                        .library
                        .bookmarks_of(&clip.path)
                        .iter()
                        .map(|b| b.id)
                        .collect();
                    for b in &all {
                        if widgets::bookmark_pill(ui, &b.name, b.color, mine.contains(&b.id), false)
                            .clicked()
                        {
                            app.library.toggle_bookmark(&clip.path, b.id);
                        }
                    }
                    let label = if all.is_empty() {
                        "Create a bookmark"
                    } else {
                        "Edit bookmarks"
                    };
                    if widgets::quiet_button(ui, None, label).clicked() {
                        app.sheet.manage = true;
                    }
                });

                ui.add_space(2.0);
                field_label(ui, "Folder");
                folder_picker(app, ui, &clip);

                ui.add_space(2.0);
                field_label(ui, "Note");
                egui::Frame::none()
                    .fill(p.card_hi)
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        let r = ui.add(
                            egui::TextEdit::multiline(&mut app.sheet.note_buf)
                                .hint_text("Add a note\u{2026}")
                                .frame(false)
                                .desired_rows(4)
                                .desired_width(f32::INFINITY),
                        );
                        if r.changed() {
                            app.library.set_note(&clip.path, &app.sheet.note_buf);
                        }
                    });

                if let Some(e) = app.sheet.error.clone() {
                    widgets::notice(ui, p.red, &e);
                }
                ui.add_space(4.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::primary_button(ui, None, "Done").clicked() {
                        done = true;
                    }
                });
            });
        });
    });

    if close || done {
        if !done {
            rename(app, &clip);
        }
        let _ = app.library.save();
        app.sheet.open = None;
    }
}

fn clip_addons(app: &mut ScApp, ui: &mut Ui, clip: &Clip) {
    let list = app.settings.addon.buttons_at("clip");
    if list.is_empty() {
        return;
    }
    let job = app.settings.addon.running().cloned();
    let at = job.as_ref().and_then(|j| {
        list.iter()
            .position(|h| j.is(h) && j.clip.as_deref() == Some(clip.path.as_path()))
    });
    if let Some(h) = addon_row(ui, &list, job.is_some(), at).and_then(|i| list.get(i)) {
        start_addon(app, h, Some(clip.path.clone()), ui.ctx());
    }
}

fn rename(app: &mut ScApp, clip: &Clip) {
    let current = app.sheet.open.clone().unwrap_or_else(|| clip.path.clone());
    let stem = current
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let wanted = app.sheet.name_buf.trim().to_string();
    if wanted.is_empty() {
        app.sheet.name_buf = stem;
        return;
    }
    if library::clean_name(&wanted) == stem {
        return;
    }
    match library::rename_clip(&current, &wanted) {
        Ok(new) => {
            app.library.set_game_if_unset(&current, &clip.game);
            after_file_change(app, &current, &new);
            app.sheet.name_buf = new
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            app.sheet.error = None;
            app.toast("Renamed", ToastKind::Ok);
        }
        Err(e) => app.sheet.error = Some(e),
    }
}

fn after_file_change(app: &mut ScApp, old: &std::path::Path, new: &std::path::Path) {
    app.library.rekey(old, new);
    if app.gallery.opened.as_deref() == Some(old) {
        app.gallery.opened = Some(new.to_path_buf());
    }
    app.thumbs.forget(old);
    app.gallery.rescan();
    if app.sheet.open.as_deref() == Some(old) {
        app.sheet.open = Some(new.to_path_buf());
        app.sheet.loaded_for = Some(new.to_path_buf());
    }
}

pub fn move_clip_to(app: &mut ScApp, clip: &Clip, dest: &std::path::Path) {
    let root = app.cfg.save.resolve_dir();
    match library::move_clip(&clip.path, dest) {
        Ok(new) => {
            after_file_change(app, &clip.path, &new);
            app.toast(
                format!("Moved to {}", gallery::folder_label(dest, &root)),
                ToastKind::Ok,
            );
        }
        Err(e) => app.toast(e, ToastKind::Error),
    }
}

pub fn move_clip_to_new_folder(app: &mut ScApp, clip: &Clip) {
    app.sheet.folder_dialog = Some(FolderDialog {
        name: String::new(),
        move_clip: Some(clip.path.clone()),
    });
}

pub(crate) fn delete_clip(app: &mut ScApp, clip: &Clip) {
    if gallery::delete(&clip.path) {
        app.library.forget(&clip.path);
        app.toast("Moved to the Recycle Bin", ToastKind::Ok);
    } else {
        app.toast("Couldn’t delete that file", ToastKind::Error);
    }
    app.thumbs.forget(&clip.path);
    app.gallery.rescan();
}

fn folder_picker(app: &mut ScApp, ui: &mut Ui, clip: &Clip) {
    let root = app.cfg.save.resolve_dir();
    let current_dir = clip
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let folders = app.gallery.folder_choices(&root);
    let label = gallery::folder_label(&current_dir, &root);
    let mut target: Option<PathBuf> = None;
    let mut new_folder = false;
    egui::ComboBox::from_id_salt("move-folder")
        .selected_text(label)
        .width(ui.available_width() - 4.0)
        .show_ui(ui, |ui| {
            for f in &folders {
                let on = *f == current_dir;
                if ui
                    .selectable_label(on, gallery::folder_label(f, &root))
                    .clicked()
                    && !on
                {
                    target = Some(f.clone());
                }
            }
            ui.separator();
            if ui.selectable_label(false, "New folder\u{2026}").clicked() {
                new_folder = true;
            }
        });
    if let Some(dest) = target {
        match library::move_clip(&clip.path, &dest) {
            Ok(new) => {
                after_file_change(app, &clip.path, &new);
                app.sheet.error = None;
                app.toast(
                    format!("Moved to {}", gallery::folder_label(&dest, &root)),
                    ToastKind::Ok,
                );
            }
            Err(e) => app.sheet.error = Some(e),
        }
    }
    if new_folder {
        app.sheet.folder_dialog = Some(FolderDialog {
            name: String::new(),
            move_clip: Some(clip.path.clone()),
        });
    }
}

fn manager(app: &mut ScApp, ctx: &egui::Context) {
    let mut done = false;
    let close = modal(ctx, "bookmark-manager", 500.0, |ui| {
        let p = theme::pal_ui(ui);
        ui.spacing_mut().item_spacing.y = 10.0;
        title(ui, "Bookmarks");
        widgets::secondary(
            ui,
            "Name and colour your own bookmarks, then tag clips with them. They also show up as a way to browse your clips.",
        );
        ui.add_space(4.0);

        let snapshot = app.library.bookmarks.clone();
        let mut edits: Vec<(u32, String, [u8; 3])> = Vec::new();
        let mut remove: Option<u32> = None;
        if snapshot.is_empty() {
            ui.label(RichText::new("No bookmarks yet. Make your first one below.").color(p.text_2));
        }
        for b in &snapshot {
            ui.horizontal(|ui| {
                let mut name = b.name.clone();
                let r = text_box(ui, &mut name, "Name", 150.0);
                let mut color = b.color;
                if let Some(i) = widgets::swatches(ui, &COLORS, Some(b.color)) {
                    color = COLORS[i];
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::icon_button(
                        ui,
                        icon::DELETE,
                        "Delete",
                        "Delete this bookmark",
                        true,
                    )
                    .clicked()
                    {
                        remove = Some(b.id);
                    }
                    let n = app.library.bookmark_uses(b.id);
                    ui.label(
                        RichText::new(if n == 1 {
                            "1 clip".into()
                        } else {
                            format!("{n} clips")
                        })
                        .size(12.0)
                        .color(p.text_3),
                    );
                });
                let trimmed = name.trim().to_string();
                if (r.changed() || color != b.color) && !trimmed.is_empty() {
                    edits.push((b.id, trimmed, color));
                }
            });
        }
        for (id, name, color) in edits {
            app.library.edit_bookmark(id, &name, color);
        }
        if let Some(id) = remove {
            app.library.delete_bookmark(id);
        }

        ui.add_space(6.0);
        field_label(ui, "New bookmark");
        let mut add = false;
        ui.horizontal(|ui| {
            let r = text_box(
                ui,
                &mut app.sheet.new_name,
                "e.g. Clutch, Funny, Fail",
                190.0,
            );
            if let Some(i) = widgets::swatches(ui, &COLORS, Some(COLORS[app.sheet.new_color])) {
                app.sheet.new_color = i;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                add = true;
            }
        });
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, None, "Add bookmark").clicked() {
                add = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::quiet_button(ui, None, "Done").clicked() {
                    done = true;
                }
            });
        });
        if add && !app.sheet.new_name.trim().is_empty() {
            let color = COLORS[app.sheet.new_color];
            app.library.add_bookmark(&app.sheet.new_name, color);
            app.sheet.new_name.clear();
            app.sheet.new_color = (app.sheet.new_color + 1) % COLORS.len();
        }
    });
    if close || done {
        let _ = app.library.save();
        app.sheet.manage = false;
    }
}

fn folder_dialog(app: &mut ScApp, ctx: &egui::Context) {
    let mut create = false;
    let mut cancel = false;
    let close = modal(ctx, "new-folder", 400.0, |ui| {
        let p = theme::pal_ui(ui);
        ui.spacing_mut().item_spacing.y = 10.0;
        title(ui, "New folder");
        let moving = app
            .sheet
            .folder_dialog
            .as_ref()
            .and_then(|d| d.move_clip.as_ref())
            .is_some();
        widgets::secondary(
            ui,
            if moving {
                "The clip will be moved into it."
            } else {
                "Folders live inside your clips folder."
            },
        );
        if let Some(d) = app.sheet.folder_dialog.as_mut() {
            let r = text_box(ui, &mut d.name, "Folder name", ui.available_width());
            r.request_focus();
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
        }
        if let Some(e) = app.sheet.error.clone() {
            widgets::notice(ui, p.red, &e);
        }
        ui.horizontal(|ui| {
            if widgets::quiet_button(ui, None, "Cancel").clicked() {
                cancel = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::primary_button(
                    ui,
                    None,
                    if moving { "Create and move" } else { "Create" },
                )
                .clicked()
                {
                    create = true;
                }
            });
        });
    });
    if create {
        make_folder(app);
    }
    if close || cancel {
        app.sheet.folder_dialog = None;
        app.sheet.error = None;
    }
}

fn make_folder(app: &mut ScApp) {
    let Some(d) = app.sheet.folder_dialog.take() else {
        return;
    };
    let root = app.cfg.save.resolve_dir();
    match library::create_folder(&root, &d.name) {
        Ok(dir) => {
            app.sheet.error = None;
            app.gallery.rescan();
            if let Some(clip) = d.move_clip {
                match library::move_clip(&clip, &dir) {
                    Ok(new) => {
                        after_file_change(app, &clip, &new);
                        app.toast("Created the folder and moved the clip", ToastKind::Ok);
                    }
                    Err(e) => app.sheet.error = Some(e),
                }
            } else {
                app.toast("Folder created", ToastKind::Ok);
            }
        }
        Err(e) => {
            app.sheet.error = Some(e);
            app.sheet.folder_dialog = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Harness;
    use egui::{Event, Key, Modifiers};

    fn meme() -> Home {
        let mut button = addon::meme_button();
        button.ask[0].hint = "Top line".into();
        button.ask[1].hint = "Bottom line".into();
        Home {
            addon: "meme".into(),
            exe: PathBuf::from("sc-meme.exe"),
            button,
        }
    }

    fn typed(text: &str) -> Vec<Event> {
        vec![Event::Text(text.into())]
    }

    #[test]
    fn typed_answers_go_out_with_the_primary_button() {
        let mut d = AskDialog::new(meme(), Some(PathBuf::from("a.mp4")), Vec::new());
        let mut out = None;
        let mut h = Harness::new();
        {
            let mut f = |ui: &mut Ui| {
                if let Some(r) = ask_form(ui, &mut d) {
                    out = Some(r);
                }
            };
            h.frame(Vec::new(), Modifiers::NONE, &mut f);
            h.frame(typed(&"x".repeat(100)), Modifiers::NONE, &mut f);
            h.click_text("Bottom line", &mut f);
            h.frame(typed("actually hits"), Modifiers::NONE, &mut f);
            h.click_nth("Meme captions", 1, &mut f);
        }
        assert_eq!(out, Some(true));
        assert_eq!(
            d.answers(),
            vec![
                ("top".to_string(), "x".repeat(80)),
                ("bottom".to_string(), "actually hits".to_string()),
            ]
        );
    }

    #[test]
    fn enter_sends_the_answers() {
        let mut d = AskDialog::new(meme(), None, Vec::new());
        let mut out = None;
        let mut h = Harness::new();
        {
            let mut f = |ui: &mut Ui| {
                if let Some(r) = ask_form(ui, &mut d) {
                    out = Some(r);
                }
            };
            h.frame(Vec::new(), Modifiers::NONE, &mut f);
            h.frame(typed("When the clip"), Modifiers::NONE, &mut f);
            h.key(Key::Enter, Modifiers::NONE, &mut f);
        }
        assert_eq!(out, Some(true));
        assert_eq!(d.answers()[0].1, "When the clip");
        assert_eq!(d.answers()[1].1, "");
    }

    #[test]
    fn nothing_typed_means_nothing_to_send() {
        let mut d = AskDialog::new(meme(), None, vec!["  ".into()]);
        let out = std::cell::Cell::new(None);
        let mut h = Harness::new();
        {
            let mut f = |ui: &mut Ui| {
                if let Some(r) = ask_form(ui, &mut d) {
                    out.set(Some(r));
                }
            };
            h.frame(Vec::new(), Modifiers::NONE, &mut f);
            h.click_nth("Meme captions", 1, &mut f);
            assert_eq!(out.get(), None);
            h.key(Key::Enter, Modifiers::NONE, &mut f);
            assert_eq!(out.get(), None);
            h.click_text("Cancel", &mut f);
        }
        assert_eq!(out.get(), Some(false));
        assert!(!d.ready());
    }

    #[test]
    fn answers_are_remembered_per_button() {
        let mut s = SheetState::default();
        s.ask_for(meme(), None);
        let mut d = s.ask.take().unwrap();
        assert_eq!(d.values, vec![String::new(), String::new()]);
        d.values[0] = "Top".into();
        s.remember(&d);
        s.ask_for(meme(), Some(PathBuf::from("b.mp4")));
        assert_eq!(s.ask.as_ref().unwrap().values[0], "Top");
        let mut other = meme();
        other.button.id = "other".into();
        s.ask_for(other, None);
        assert_eq!(s.ask.as_ref().unwrap().values[0], "");
    }

    #[test]
    fn a_running_clip_button_shows_its_busy_text() {
        let list = vec![meme()];
        let mut hit = None;
        let mut h = Harness::new();
        let mut idle = |ui: &mut Ui| {
            if let Some(i) = addon_row(ui, &list, false, None) {
                hit = Some(i);
            }
        };
        h.frame(Vec::new(), Modifiers::NONE, &mut idle);
        h.click_text("Meme captions", &mut idle);
        assert_eq!(hit, Some(0));
        hit = None;
        let mut other = |ui: &mut Ui| {
            if let Some(i) = addon_row(ui, &list, true, None) {
                hit = Some(i);
            }
        };
        h.frame(Vec::new(), Modifiers::NONE, &mut other);
        h.click_text("Meme captions", &mut other);
        assert_eq!(hit, None);
        h.frame(Vec::new(), Modifiers::NONE, &mut |ui| {
            addon_row(ui, &list, true, Some(0));
        });
        assert!(h.has("Making your meme\u{2026}"));
        assert!(!h.has("Meme captions"));
        let mut plain = meme();
        plain.button.busy.clear();
        assert_eq!(busy_text(&plain), "Working\u{2026}");
    }
}
