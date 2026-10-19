use crate::app::{ScApp, Screen, ToastKind};
use crate::theme::{self, icon, pal_ui};
use crate::{daemon, widgets};
use egui::{vec2, Align, FontId, Layout, RichText, Rounding, Ui};
use sc_core::ipc::{Request, Response};
use std::path::{Path, PathBuf};

/// Per-clip trim state, kept in the app so handle positions survive repaints.
#[derive(Default)]
pub struct TrimState {
    path: PathBuf,
    duration: Option<f64>,
    error: Option<String>,
    start: f64,
    end: f64,
    /// Result of the last export, shown under the controls.
    result: Option<PathBuf>,
}

impl TrimState {
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

/// Lossless trim: pick a start and end, and the daemon cuts the clip without
/// re-encoding. The cut starts on the keyframe at or before the chosen start
/// (keyframes are ~2 s apart), so it may begin slightly earlier.
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

    // Preview frame.
    let w = ui.available_width().min(640.0);
    let size = vec2(w, (w * 9.0 / 16.0).round());
    let clip = crate::screens::gallery::Clip {
        path: path.to_path_buf(),
        name: name.clone(),
        modified: std::time::SystemTime::UNIX_EPOCH,
        bytes: 0,
        is_image: false,
        game: String::new(),
    };
    let resp = crate::screens::gallery::preview(app, ui, &clip, size);
    if resp.on_hover_text("Play the original").clicked() {
        crate::screens::gallery::open_path(path);
    }
    ui.add_space(14.0);

    let mut export = false;
    widgets::card(ui, |ui| {
        let t = &mut app.trim;
        widgets::range_slider(ui, &mut t.start, &mut t.end, duration, 0.5);
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
        ui.horizontal(|ui| {
            export = widgets::primary_button(ui, Some(icon::CUT), "Save trimmed copy").clicked();
            ui.label(
                RichText::new("The original is kept. No re-encoding, so quality is unchanged.")
                    .size(12.5)
                    .color(p.text_2),
            );
        });
    });
    if export {
        let req = Request::Trim {
            path: path.to_path_buf(),
            start_secs: app.trim.start,
            end_secs: app.trim.end,
        };
        match daemon::request(req) {
            Ok(Response::Saved {
                path,
                duration_secs,
            }) => {
                app.trim.result = Some(path);
                app.toast(format!("Saved a {duration_secs:.1} s copy"), ToastKind::Ok);
            }
            Ok(Response::Error { message }) => app.toast(message, ToastKind::Warn),
            Ok(_) => {}
            Err(e) => app.toast(e.to_string(), ToastKind::Error),
        }
        app.gallery.rescan();
    }
    ui.add_space(8.0);
    widgets::secondary(
        ui,
        "Cuts start on the nearest earlier keyframe (up to about 2 s early) to stay lossless.",
    );
    let _ = Rounding::ZERO;
}
