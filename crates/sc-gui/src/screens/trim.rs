use crate::app::{ScApp, Screen, ToastKind};
use crate::theme::{self, icon, pal_ui};
use crate::{daemon, widgets};
use egui::{Align, FontId, Layout, RichText, Ui};
use sc_core::ipc::{Request, Response, TrimDest};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Default)]
enum SaveMode {
    #[default]
    Copy,
    Replace,
}

#[derive(Default)]
pub struct TrimState {
    path: PathBuf,
    duration: Option<f64>,
    error: Option<String>,
    start: f64,
    end: f64,
    mode: SaveMode,
    result: Option<PathBuf>,
    #[cfg(windows)]
    player: Option<crate::player::Player>,
}

impl TrimState {
    pub fn stop_player(&mut self) {
        #[cfg(windows)]
        {
            self.player = None;
        }
    }

    pub fn demo_select(&mut self, start: f64, end: f64) {
        self.start = start;
        self.end = end.min(self.duration.unwrap_or(end));
        #[cfg(windows)]
        if let Some(p) = self.player.as_mut() {
            p.set_range(self.start, self.end);
            p.focus(self.start);
        }
    }

    pub fn demo_play(&mut self) {
        #[cfg(windows)]
        if let Some(p) = self.player.as_mut() {
            p.toggle();
        }
    }

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
                #[cfg(windows)]
                {
                    let mut p = crate::player::Player::open_paused(path, 0.0);
                    p.set_range(0.0, duration_secs);
                    self.player = Some(p);
                }
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

    #[cfg(windows)]
    if let Some(pl) = app.trim.player.as_mut() {
        pl.set_range(app.trim.start, app.trim.end);
        pl.show(ui);
    }
    #[cfg(not(windows))]
    widgets::secondary(ui, "The live trim preview needs Windows.");
    ui.add_space(10.0);

    let mut export = false;
    widgets::card(ui, |ui| {
        let t = &mut app.trim;
        let (s0, e0) = (t.start, t.end);
        widgets::range_slider(ui, &mut t.start, &mut t.end, duration, 0.5);
        #[cfg(windows)]
        if let Some(pl) = t.player.as_mut() {
            let (s, e) = (t.start, t.end);
            if (s - s0).abs() > f64::EPSILON {
                pl.set_range(s, e);
                pl.focus(s);
            } else if (e - e0).abs() > f64::EPSILON {
                pl.set_range(s, e);
                pl.focus((e - 0.1).max(s));
            }
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
            let label = match t.mode {
                SaveMode::Copy => "Save trimmed copy",
                SaveMode::Replace => "Replace original",
            };
            export = widgets::primary_button(ui, Some(icon::CUT), label).clicked();
        });
    });

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
        "Playback shows your exact selection. The saved cut starts on the nearest earlier keyframe (up to about 2 s early) to stay lossless.",
    );
}

fn export_trim(app: &mut ScApp, original: &Path) {
    let replace = app.trim.mode == SaveMode::Replace;
    let req = Request::Trim {
        path: original.to_path_buf(),
        start_secs: app.trim.start,
        end_secs: app.trim.end,
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
        if app.cfg.general.share_prompt {
            app.share = Some(crate::screens::share::ShareState::new(new_path.clone()));
        }
        app.trim.result = Some(new_path);
        app.gallery.rescan();
        app.toast(format!("Saved a {secs:.1} s copy"), ToastKind::Ok);
        return;
    }

    app.trim.stop_player();
    if !crate::screens::gallery::delete(original) {
        let _ = std::fs::remove_file(&new_path);
        app.toast(
            "Couldn’t move the original to the Recycle Bin, so nothing was changed",
            ToastKind::Error,
        );
        return;
    }
    let final_path = match std::fs::rename(&new_path, original) {
        Ok(()) => original.to_path_buf(),
        Err(_) => new_path,
    };
    app.library.rekey_if_missing(original, &final_path);
    app.library.set_trimmed(&final_path, true);
    if app.cfg.general.share_prompt {
        app.share = Some(crate::screens::share::ShareState::new(final_path.clone()));
    }
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
