use crate::app::{ScApp, Screen, ToastKind};
use crate::library::{self, COLORS};
use crate::screens::gallery::{self, Clip};
use crate::theme::{self, icon, pal};
use crate::widgets;
use egui::{vec2, Align, Align2, Color32, FontId, Layout, Order, RichText, Rounding, Sense, Ui};
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
    compress_mb: u32,
    compress_open: bool,
    compress_job: Option<crossbeam_channel::Receiver<Result<(PathBuf, f64), String>>>,
}

pub struct FolderDialog {
    pub name: String,
    pub move_clip: Option<PathBuf>,
}

impl SheetState {
    pub fn open_clip(&mut self, path: PathBuf) {
        self.open = Some(path);
        self.loaded_for = None;
        self.error = None;
    }
}

pub fn show(app: &mut ScApp, ctx: &egui::Context) {
    poll_compress(app, ctx);
    if app.sheet.folder_dialog.is_some() {
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
    let p = theme::pal_ui(ui);
    egui::Frame::none()
        .fill(p.card_hi)
        .rounding(Rounding::same(9.0))
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text(hint)
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
                if !clip.is_image && widgets::quiet_button(ui, Some(icon::CUT), "Trim").clicked() {
                    app.goto(Screen::Trim(clip.path.clone()));
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
                if !clip.is_image {
                    let busy = app.sheet.compress_job.is_some();
                    let label = if busy {
                        "Compressing\u{2026}"
                    } else {
                        "Compress for Discord\u{2026}"
                    };
                    if widgets::quiet_button(ui, Some(icon::SHARE), label).clicked() && !busy {
                        app.sheet.compress_open = !app.sheet.compress_open;
                    }
                    if app.sheet.compress_open && !busy {
                        if app.sheet.compress_mb == 0 {
                            app.sheet.compress_mb = 10;
                        }
                        widgets::segmented(
                            ui,
                            &mut app.sheet.compress_mb,
                            &[(10, "10 MB"), (25, "25 MB"), (50, "50 MB")],
                        );
                        widgets::secondary(
                            ui,
                            "Saves a smaller copy that fits the upload limit. The original stays.",
                        );
                        if widgets::quiet_button(ui, None, "Compress").clicked() {
                            start_compress(app, &clip);
                        }
                    }
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

pub fn compress_clip(app: &mut ScApp, clip: &Clip, mb: u32) {
    if app.sheet.compress_job.is_some() {
        app.toast("Already compressing a clip", ToastKind::Warn);
        return;
    }
    app.sheet.compress_mb = mb;
    start_compress(app, clip);
    app.toast(format!("Compressing to fit {mb} MB\u{2026}"), ToastKind::Ok);
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

fn start_compress(app: &mut ScApp, clip: &Clip) {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let req = sc_core::ipc::Request::Compress {
        path: clip.path.clone(),
        target_mb: app.sheet.compress_mb.max(1),
    };
    app.sheet.compress_job = Some(rx);
    app.sheet.compress_open = false;
    std::thread::spawn(move || {
        let result = match crate::daemon::request(req) {
            Ok(sc_core::ipc::Response::Saved {
                path,
                duration_secs,
            }) => Ok((path, duration_secs)),
            Ok(sc_core::ipc::Response::Error { message }) => Err(message),
            Ok(_) => Err("unexpected reply from SimpleClip".to_string()),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(result);
    });
}

fn poll_compress(app: &mut ScApp, ctx: &egui::Context) {
    let Some(rx) = &app.sheet.compress_job else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok((path, _))) => {
            app.sheet.compress_job = None;
            app.library.set_compressed(&path, true);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            app.gallery.rescan();
            app.toast(
                format!("Saved {name} ({})", gallery::size_label(size)),
                ToastKind::Ok,
            );
        }
        Ok(Err(message)) => {
            app.sheet.compress_job = None;
            app.toast(message, ToastKind::Warn);
        }
        Err(crossbeam_channel::TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            app.sheet.compress_job = None;
        }
    }
}
