use crate::app::{ScApp, Screen};
use crate::{daemon, theme, widgets};
use sc_core::ipc::{Request, Response};
use std::path::{Path, PathBuf};

/// Per-clip trim state, kept in the app so slider positions survive repaints.
#[derive(Default)]
pub struct TrimState {
    path: PathBuf,
    duration: Option<f64>,
    error: Option<String>,
    start: f64,
    end: f64,
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
            Ok(_) => self.error = Some("unexpected reply from daemon".into()),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
}

/// Lossless trim: pick a start and end, and the daemon cuts the clip without
/// re-encoding. The cut starts on the keyframe at or before the chosen start
/// (keyframes are ~2 s apart), so it may begin slightly earlier.
pub fn view(app: &mut ScApp, ui: &mut egui::Ui, path: &Path) {
    if app.trim.path != path {
        app.trim.load(path);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("\u{2039} Back to gallery").clicked() {
            app.screen = Screen::Gallery;
        }
        ui.heading("Trim");
    });
    ui.colored_label(theme::MUTED, path.display().to_string());
    ui.separator();

    if let Some(err) = &app.trim.error {
        widgets::card(ui, "Can't read this clip", |ui| {
            ui.colored_label(theme::WARN, err.clone());
            ui.colored_label(theme::MUTED, "The daemon must be running to trim.");
            if ui.button("Retry").clicked() {
                app.trim.path = PathBuf::new();
            }
        });
        return;
    }
    let Some(duration) = app.trim.duration else {
        ui.label("Reading clip...");
        return;
    };

    widgets::card(ui, "Preview", |ui| {
        if ui.button("Play original").clicked() {
            super::gallery::open_path(path);
        }
        ui.colored_label(
            theme::MUTED,
            "Opens in your default player. Note the times you want, then set them below.",
        );
    });
    ui.add_space(10.0);

    widgets::card(ui, "In / out points", |ui| {
        let t = &mut app.trim;
        ui.add(
            egui::Slider::new(&mut t.start, 0.0..=duration)
                .text("start (s)")
                .fixed_decimals(1),
        );
        ui.add(
            egui::Slider::new(&mut t.end, 0.0..=duration)
                .text("end (s)")
                .fixed_decimals(1),
        );
        // Keep the range valid whichever handle moved last.
        if t.end < t.start + 0.5 {
            t.end = (t.start + 0.5).min(duration);
            t.start = (t.end - 0.5).max(0.0);
        }
        ui.colored_label(
            theme::MUTED,
            format!(
                "keeps {:.1}s of {:.1}s. Lossless: the cut starts at the keyframe at or before the start point.",
                t.end - t.start,
                duration
            ),
        );
        ui.add_space(8.0);
        if ui.button("Export trimmed clip").clicked() {
            let req = Request::Trim {
                path: path.to_path_buf(),
                start_secs: t.start,
                end_secs: t.end,
            };
            match daemon::request(req) {
                Ok(Response::Saved {
                    path,
                    duration_secs,
                }) => app.toast(
                    format!("saved {} ({duration_secs:.1}s)", path.display()),
                    theme::OK,
                ),
                Ok(Response::Error { message }) => app.toast(message, theme::WARN),
                Ok(_) => {}
                Err(e) => app.toast(e.to_string(), theme::DANGER),
            }
            app.gallery.rescan();
        }
    });
}
