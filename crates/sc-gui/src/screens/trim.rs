use crate::app::{ScApp, Screen, ToastKind};
use crate::theme::{self, icon, pal_ui};
use crate::{daemon, widgets};
use egui::{vec2, Align, Align2, FontId, Layout, RichText, Rounding, Ui};
use sc_core::ipc::{Request, Response, TrimDest};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Which end of the selection the big preview shows.
#[derive(Clone, Copy, PartialEq, Default)]
enum Show {
    #[default]
    Start,
    End,
}

/// What "save" does.
#[derive(Clone, Copy, PartialEq, Default)]
enum SaveMode {
    /// Keep the original and add a trimmed copy.
    #[default]
    Copy,
    /// Keep only the trimmed version (the original goes to the Recycle Bin).
    Replace,
}

type FrameMsg = (i64, Option<egui::ColorImage>);

/// The big preview frame, loaded in the background a moment after a handle stops moving.
struct FrameView {
    tex: Option<egui::TextureHandle>,
    /// Time (in tenths of a second) the texture shows.
    shown: Option<i64>,
    /// Time we most recently asked for, so we don't ask twice.
    asked: Option<i64>,
    wanted: i64,
    wanted_since: Instant,
    tx: crossbeam_channel::Sender<FrameMsg>,
    rx: crossbeam_channel::Receiver<FrameMsg>,
}

impl Default for FrameView {
    fn default() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            tex: None,
            shown: None,
            asked: None,
            wanted: 0,
            wanted_since: Instant::now(),
            tx,
            rx,
        }
    }
}

/// Per-clip trim state, kept in the app so handle positions survive repaints.
#[derive(Default)]
pub struct TrimState {
    path: PathBuf,
    duration: Option<f64>,
    error: Option<String>,
    start: f64,
    end: f64,
    show: Show,
    mode: SaveMode,
    frame: FrameView,
    /// A preview clip is being prepared / opened.
    previewing: Arc<AtomicBool>,
    /// Where the last trimmed copy was saved.
    result: Option<PathBuf>,
    /// The temporary preview clip, once the daemon has made it.
    preview_rx: Option<crossbeam_channel::Receiver<PathBuf>>,
    /// The in-app player, when a clip is being watched.
    #[cfg(windows)]
    player: Option<crate::player::Player>,
}

impl TrimState {
    /// Stop and close the player (leaving the screen must silence it).
    pub fn stop_player(&mut self) {
        #[cfg(windows)]
        {
            self.player = None;
        }
    }

    /// Play `path` in the app's own player (or the system player elsewhere).
    pub fn play(&mut self, path: &Path) {
        #[cfg(windows)]
        {
            self.player = Some(crate::player::Player::open(path));
        }
        #[cfg(not(windows))]
        crate::screens::gallery::open_path(path);
    }

    fn playing_in_app(&self) -> bool {
        #[cfg(windows)]
        {
            self.player.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Reset for a newly opened clip and ask the daemon for its length.
    fn load(&mut self, path: &Path) {
        *self = Self {
            path: path.to_path_buf(),
            ..Default::default()
        };
        match daemon::request(Request::Probe {
            path: path.to_path_buf(),
        }) {
            Ok(Response::Probed { duration_secs }) => {
                self.duration = Some(duration_secs);
                self.end = duration_secs;
            }
            Ok(Response::Error { message }) => self.error = Some(message),
            Ok(_) => self.error = Some("Unexpected reply from SimpleClip".into()),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

/// Lossless trim: pick a start and end, look at the frames, preview the result,
/// then save a trimmed copy or replace the original. The cut starts on the
/// keyframe at or before the chosen start (keyframes are ~2 s apart), so it may
/// begin slightly earlier.
pub fn view(app: &mut ScApp, ui: &mut Ui, path: &Path) {
    let p = pal_ui(ui);
    if app.trim.path != path {
        app.trim.load(path);
    }
    ui.horizontal(|ui| {
        if widgets::quiet_button(ui, Some(icon::BACK), "Clips").clicked() {
            app.goto(Screen::Gallery);
        }
    });
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clip")
        .to_string();
    widgets::page_title(ui, "Trim", Some(&name));

    if let Some(err) = app.trim.error.clone() {
        widgets::notice(ui, p.red, &err);
        ui.add_space(8.0);
        widgets::secondary(
            ui,
            "SimpleClip must be running to trim. Start it from Home.",
        );
        ui.add_space(10.0);
        if widgets::quiet_button(ui, Some(icon::REFRESH), "Try again").clicked() {
            app.trim.path = PathBuf::new();
        }
        return;
    }
    let Some(duration) = app.trim.duration else {
        widgets::secondary(ui, "Reading clip\u{2026}");
        return;
    };

    // A finished preview clip opens in the player.
    if let Some(rx) = &app.trim.preview_rx {
        if let Ok(clip) = rx.try_recv() {
            app.trim.preview_rx = None;
            app.trim.play(&clip);
        }
    }

    // ---- the player, or the frame at the start or end of the selection ----
    if app.trim.playing_in_app() {
        #[cfg(windows)]
        if let Some(pl) = app.trim.player.as_mut() {
            pl.show(ui);
        }
        ui.add_space(8.0);
        if widgets::quiet_button(ui, Some(icon::BACK), "Back to frames").clicked() {
            app.trim.stop_player();
        }
    } else {
        frame_preview(app, ui, path);
    }
    ui.add_space(10.0);

    // ---- the selection ----
    let mut export = false;
    let mut preview = false;
    widgets::card(ui, |ui| {
        let t = &mut app.trim;
        let (s0, e0) = (t.start, t.end);
        widgets::range_slider(ui, &mut t.start, &mut t.end, duration, 0.5);
        // The frame follows whichever handle you moved.
        if (t.start - s0).abs() > f64::EPSILON {
            t.show = Show::Start;
        } else if (t.end - e0).abs() > f64::EPSILON {
            t.show = Show::End;
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(clock(t.start)).font(FontId::new(15.0, theme::semibold())));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(clock(t.end)).font(FontId::new(15.0, theme::semibold())));
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{:.1} s selected", t.end - t.start)).color(p.text_2),
                    );
                });
            });
        });
        ui.add_space(12.0);

        // Save choice.
        ui.horizontal(|ui| {
            widgets::segmented(
                ui,
                &mut t.mode,
                &[
                    (SaveMode::Copy, "Save as a copy"),
                    (SaveMode::Replace, "Replace the original"),
                ],
            );
        });
        ui.add_space(4.0);
        widgets::secondary(
            ui,
            match t.mode {
                SaveMode::Copy => "Keeps the original and adds a trimmed copy.",
                SaveMode::Replace => {
                    "Keeps only the trimmed version. The original goes to the Recycle Bin, so you can still get it back."
                }
            },
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let label = match t.mode {
                SaveMode::Copy => "Save trimmed copy",
                SaveMode::Replace => "Replace original",
            };
            export = widgets::primary_button(ui, Some(icon::CUT), label).clicked();
            let busy = t.previewing.load(Ordering::Relaxed);
            if widgets::quiet_button(
                ui,
                Some(icon::PLAY),
                if busy {
                    "Preparing preview\u{2026}"
                } else {
                    "Preview trimmed clip"
                },
            )
            .clicked()
                && !busy
            {
                preview = true;
            }
        });
    });

    if preview {
        start_preview(app, ui.ctx().clone(), path);
    }
    if export {
        export_trim(app, path);
    }

    if let Some(r) = app.trim.result.clone() {
        ui.add_space(10.0);
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Saved {}",
                        r.file_name().and_then(|n| n.to_str()).unwrap_or("copy")
                    ))
                    .color(p.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::quiet_button(ui, None, "See trimmed clips").clicked() {
                        app.gallery.show_trimmed();
                        app.goto(Screen::Gallery);
                    }
                    if widgets::quiet_button(ui, Some(icon::PLAY), "Play").clicked() {
                        app.trim.play(&r);
                    }
                });
            });
        });
    }
    ui.add_space(8.0);
    widgets::secondary(
        ui,
        match &app.cfg.save.trim_directory {
            Some(d) => format!("Trimmed copies are saved to {}.", d.display()),
            None => "Trimmed copies are saved next to the original. Pick a folder in Settings."
                .to_string(),
        },
    );
    widgets::secondary(
        ui,
        "Cuts start on the nearest earlier keyframe (up to about 2 s early) to stay lossless.",
    );
}

/// The large preview: the frame at the start or end of the selection.
fn frame_preview(app: &mut ScApp, ui: &mut Ui, path: &Path) {
    let p = pal_ui(ui);
    let t = &mut app.trim;

    // Pick up a finished load.
    while let Ok((tenths, img)) = t.frame.rx.try_recv() {
        if let Some(img) = img {
            t.frame.tex = Some(ui.ctx().load_texture(
                format!("trim-frame-{tenths}"),
                img,
                egui::TextureOptions::LINEAR,
            ));
            t.frame.shown = Some(tenths);
        }
    }

    // What we want to see, in tenths of a second.
    let secs = match t.show {
        Show::Start => t.start,
        Show::End => (t.end - 0.05).max(t.start),
    };
    let wanted = (secs * 10.0).round() as i64;
    if wanted != t.frame.wanted {
        t.frame.wanted = wanted;
        t.frame.wanted_since = Instant::now();
    }
    // Ask once the handle has been still for a moment (not on every drag pixel).
    let settled = t.frame.wanted_since.elapsed() > Duration::from_millis(140);
    if settled && t.frame.shown != Some(wanted) && t.frame.asked != Some(wanted) {
        t.frame.asked = Some(wanted);
        let (tx, ctx, p) = (t.frame.tx.clone(), ui.ctx().clone(), path.to_path_buf());
        std::thread::spawn(move || {
            let img = match daemon::request(Request::Frame {
                path: p,
                secs: wanted as f64 / 10.0,
                width: 960,
            }) {
                Ok(Response::Thumbnail { path }) => crate::thumbs::decode_png(&path),
                _ => None,
            };
            let _ = tx.send((wanted, img));
            ctx.request_repaint();
        });
    } else if !settled || t.frame.shown != Some(wanted) {
        ui.ctx().request_repaint_after(Duration::from_millis(60));
    }

    let w = ui.available_width().min(640.0);
    let size = vec2(w, (w * 9.0 / 16.0).round());
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    match &t.frame.tex {
        Some(tex) => {
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                .fit_to_exact_size(size)
                .rounding(Rounding::same(10.0))
                .paint_at(ui, rect);
        }
        None => {
            ui.painter()
                .rect_filled(rect, Rounding::same(10.0), p.card_hi);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Loading frame\u{2026}",
                FontId::new(12.0, egui::FontFamily::Proportional),
                p.text_3,
            );
        }
    }
    // Which time this is, in the corner.
    let label = format!(
        "{} {}",
        if t.show == Show::Start {
            "Start"
        } else {
            "End"
        },
        clock(secs)
    );
    let galley = ui.painter().layout_no_wrap(
        label,
        FontId::new(12.0, theme::semibold()),
        egui::Color32::WHITE,
    );
    let tag = egui::Rect::from_min_size(
        rect.left_bottom() + vec2(10.0, -10.0 - galley.size().y - 8.0),
        galley.size() + vec2(16.0, 8.0),
    );
    ui.painter().rect_filled(
        tag,
        Rounding::same(tag.height() / 2.0),
        egui::Color32::from_black_alpha(150),
    );
    ui.painter().galley(
        tag.center() - galley.size() / 2.0,
        galley,
        egui::Color32::WHITE,
    );
    if resp.on_hover_text("Play the original").clicked() {
        t.play(path);
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        widgets::segmented(
            ui,
            &mut t.show,
            &[(Show::Start, "Start frame"), (Show::End, "End frame")],
        );
    });
}

/// Make a temporary trimmed clip and open it in the default player, so it can
/// be watched before committing to anything.
fn start_preview(app: &mut ScApp, ctx: egui::Context, path: &Path) {
    let flag = app.trim.previewing.clone();
    flag.store(true, Ordering::Relaxed);
    let req = Request::Trim {
        path: path.to_path_buf(),
        start_secs: app.trim.start,
        end_secs: app.trim.end,
        dest: TrimDest::Temp,
    };
    let (tx, rx) = crossbeam_channel::bounded(1);
    app.trim.preview_rx = Some(rx);
    std::thread::spawn(move || {
        if let Ok(Response::Saved { path, .. }) = daemon::request(req) {
            let _ = tx.send(path);
        }
        flag.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

/// Save the trim as a copy, or replace the original with it.
fn export_trim(app: &mut ScApp, original: &Path) {
    let replace = app.trim.mode == SaveMode::Replace;
    let req = Request::Trim {
        path: original.to_path_buf(),
        start_secs: app.trim.start,
        end_secs: app.trim.end,
        // Replacing needs the result beside the original so it can take its place.
        dest: if replace {
            TrimDest::Beside
        } else {
            TrimDest::Default
        },
    };
    let (new_path, secs) = match daemon::request(req) {
        Ok(Response::Saved {
            path,
            duration_secs,
        }) => (path, duration_secs),
        Ok(Response::Error { message }) => {
            app.toast(message, ToastKind::Warn);
            return;
        }
        Ok(_) => return,
        Err(e) => {
            app.toast(e.to_string(), ToastKind::Error);
            return;
        }
    };

    if !replace {
        app.library.set_trimmed(&new_path, true);
        app.trim.result = Some(new_path);
        app.gallery.rescan();
        app.toast(format!("Saved a {secs:.1} s copy"), ToastKind::Ok);
        return;
    }

    // Replace: the original goes to the Recycle Bin and the trimmed clip takes
    // its name, so its bookmarks, note and place in the list carry over.
    if !crate::screens::gallery::delete(original) {
        let _ = std::fs::remove_file(&new_path);
        app.toast(
            "Couldn't move the original to the Recycle Bin, so nothing was changed",
            ToastKind::Error,
        );
        return;
    }
    let final_path = match std::fs::rename(&new_path, original) {
        Ok(()) => original.to_path_buf(),
        Err(_) => new_path, // keep the trimmed file under its own name
    };
    app.library.rekey_if_missing(original, &final_path);
    app.library.set_trimmed(&final_path, true);
    app.thumbs.forget(original);
    app.thumbs.forget(&final_path);
    app.gallery.rescan();
    app.trim.path = PathBuf::new();
    app.toast(
        format!("Trimmed to {secs:.1} s. The original is in the Recycle Bin"),
        ToastKind::Ok,
    );
    app.goto(Screen::Gallery);
}
