use crate::app::{ScApp, Screen, ToastKind};
use crate::theme::{self, pal_ui};
use crate::{config_io, widgets};
use egui::{pos2, vec2, Align, Color32, FontId, Layout, RichText, Sense, Ui};

const STEPS: usize = 5;

#[derive(Default)]
pub struct WizardState {
    pub step: usize,
}

const FOOTER: f32 = 64.0;

pub fn view(app: &mut ScApp, ui: &mut Ui) {
    app.devices.ensure(&app.link);
    let step = app.wizard.step;
    frame(
        ui,
        step,
        app,
        |app, ui| match app.wizard.step {
            0 => welcome(ui),
            1 => look(app, ui),
            2 => recording(app, ui),
            3 => shortcut(app, ui),
            _ => ready(app, ui),
        },
        buttons,
    );
}

fn frame<T>(
    ui: &mut Ui,
    step: usize,
    state: &mut T,
    body: impl FnOnce(&mut T, &mut Ui),
    footer: impl FnOnce(&mut T, &mut Ui),
) {
    let avail = ui.available_size();
    let w = 600.0_f32.min(avail.x - 48.0);
    ui.vertical_centered(|ui| {
        ui.add_space(((avail.y - 520.0) / 2.0).clamp(18.0, 80.0));
        let remaining = ui.available_height();
        ui.allocate_ui_with_layout(vec2(w, remaining), Layout::top_down(Align::Min), |ui| {
            let room = (remaining - FOOTER - 26.0).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt(("wizard-step", step))
                .max_height(room)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.set_width(w);
                    progress(ui, step);
                    ui.add_space(26.0);
                    body(state, ui);
                });
            ui.add_space(26.0);
            footer(state, ui);
        });
    });
}

fn progress(ui: &mut Ui, step: usize) {
    let p = pal_ui(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for i in 0..STEPS {
            let (w, color) = dot(&p, i, step);
            let (rect, _) = ui.allocate_exact_size(vec2(w, 8.0), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, color);
        }
    });
}

fn dot(p: &theme::Palette, i: usize, step: usize) -> (f32, Color32) {
    if i == step {
        (26.0, p.accent)
    } else if i < step {
        (8.0, p.accent.linear_multiply(0.5))
    } else {
        (8.0, p.line)
    }
}

fn heading(ui: &mut Ui, title: &str, sub: &str) {
    let p = pal_ui(ui);
    ui.label(
        RichText::new(title)
            .font(FontId::new(32.0, theme::semibold()))
            .color(p.text),
    );
    ui.add_space(4.0);
    ui.label(RichText::new(sub).size(15.0).color(p.text_2));
    ui.add_space(20.0);
}

fn welcome(ui: &mut Ui) {
    let p = pal_ui(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::Rounding::same(16.0),
        Color32::from_rgb(0x1f, 0x21, 0x2b),
    );
    ui.painter().circle_stroke(
        rect.center(),
        16.0,
        egui::Stroke::new(4.0_f32, Color32::from_rgb(0xee, 0xf0, 0xf6)),
    );
    ui.painter()
        .circle_filled(rect.center(), 8.0, Color32::from_rgb(0xe5, 0x39, 0x35));
    ui.add_space(18.0);
    heading(
        ui,
        "Welcome to SimpleClip",
        "Never miss a great moment again.",
    );
    widgets::section(ui, None, |s| {
        for (title, body) in [
            (
                "Always ready",
                "It quietly keeps the last few seconds of your screen.",
            ),
            (
                "One shortcut",
                "Something amazing happens? Press a key to save it.",
            ),
            (
                "Nothing to manage",
                "No record button, no account, no uploads.",
            ),
        ] {
            s.full("", |ui| {
                ui.label(
                    RichText::new(title)
                        .font(FontId::new(15.0, theme::semibold()))
                        .color(p.text),
                );
                ui.label(RichText::new(body).size(13.5).color(p.text_2));
            });
        }
    });
}

fn look(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    heading(
        ui,
        "Choose a look",
        "Pick the colors you like. SimpleClip is the default. You can change this any time in Settings.",
    );
    let accent = app.cfg.general.accent.clone();
    widgets::theme_picker(ui, &mut app.cfg.general.theme, true, accent.as_deref());
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Accent color")
                .font(FontId::new(14.0, theme::semibold()))
                .color(p.text),
        );
        ui.add_space(8.0);
        widgets::accent_picker(ui, &mut app.cfg.general.accent);
    });
}

fn recording(app: &mut ScApp, ui: &mut Ui) {
    heading(
        ui,
        "What should it record?",
        "Sensible defaults are already chosen.",
    );
    widgets::section(ui, None, |s| {
        if cfg!(windows) && app.devices.monitors.len() > 1 {
            s.full("", |ui| {
                let p = pal_ui(ui);
                ui.label(RichText::new("Which screen?").size(14.0).color(p.text));
                ui.label(
                    RichText::new("The pictures show what each screen looks like right now.")
                        .size(12.0)
                        .color(p.text_2),
                );
                ui.add_space(10.0);
                widgets::monitor_picker(ui, &mut app.devices, &mut app.cfg.capture.monitor_id);
            });
        }
        s.row("Keep the last", Some("Length of each saved clip"), |ui| {
            let mut v = app.cfg.buffer.replay_duration_secs;
            let opts = [
                (15u32, "15 s"),
                (30, "30 s"),
                (45, "45 s"),
                (60, "1 min"),
                (120, "2 min"),
            ];
            if opts.iter().any(|(o, _)| *o == v) {
                widgets::segmented(ui, &mut v, &opts);
                app.cfg.buffer.replay_duration_secs = v;
            } else {
                widgets::segmented(
                    ui,
                    &mut app.cfg.buffer.replay_duration_secs,
                    &[(v, &format!("{v} s"))],
                );
            }
        });
        s.row("Game and app sound", None, |ui| {
            widgets::toggle(ui, &mut app.cfg.audio.desktop_enabled);
        });
        s.row("Microphone", Some("Include your voice"), |ui| {
            widgets::toggle(ui, &mut app.cfg.audio.mic_enabled);
        });
    });
}

fn shortcut(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    heading(
        ui,
        "Pick your shortcut",
        "Press it any time to save the last moments.",
    );
    widgets::section(ui, None, |s| {
        s.row("Save clip", None, |ui| {
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.save, "wiz-hk-save");
        });
        s.row("Take screenshot", None, |ui| {
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.screenshot, "wiz-hk-shot");
        });
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new("Choose something your games don’t use. You can change this later.")
            .size(13.0)
            .color(p.text_2),
    );
}

fn ready(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    heading(
        ui,
        "You’re all set",
        "SimpleClip is recording in the background.",
    );
    widgets::section(ui, None, |s| {
        s.row("Save a clip", None, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                widgets::keycaps(ui, &app.cfg.hotkeys.save);
            });
        });
        if cfg!(windows) {
            s.row(
                "Open at login",
                Some("Keeps the buffer ready after a restart"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.general.autostart);
                },
            );
        }
    });
    ui.add_space(8.0);
    ui.label(
        RichText::new("It lives in the system tray. Clips are saved to your Videos folder.")
            .size(13.0)
            .color(p.text_2),
    );
    let _ = pos2(0.0, 0.0);
}

fn buttons(app: &mut ScApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if app.wizard.step > 0 && widgets::quiet_button(ui, None, "Back").clicked() {
            app.wizard.step -= 1;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let last = app.wizard.step == STEPS - 1;
            let label = if last { "Start clipping" } else { "Continue" };
            if widgets::primary_button(ui, None, label).clicked() {
                if last {
                    finish(app);
                } else {
                    app.wizard.step += 1;
                }
            }
        });
    });
}

fn finish(app: &mut ScApp) {
    app.cfg.general.setup_complete = true;
    match config_io::save(&app.cfg) {
        Ok(()) => {
            app.screen = Screen::Dashboard;
            app.toast("You’re all set", ToastKind::Ok);
        }
        Err(e) => app.toast(format!("Couldn’t save settings: {e}"), ToastKind::Error),
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use crate::testkit::Harness;

    fn show(h: &mut Harness, lines: usize) {
        for _ in 0..3 {
            h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                frame(
                    ui,
                    2,
                    &mut (),
                    |_, ui| {
                        for i in 0..lines {
                            ui.label(format!("Line {i}"));
                            ui.add_space(14.0);
                        }
                    },
                    |_, ui| {
                        widgets::primary_button(ui, None, "Continue");
                    },
                );
            });
        }
    }

    #[test]
    fn the_continue_button_stays_inside_the_window_when_the_page_is_tall() {
        for height in [500.0, 640.0, 820.0] {
            let mut h = Harness::new().sized(1000.0, height);
            show(&mut h, 60);
            let button = h.at("Continue", 0).expect("the button is drawn");
            assert!(
                button.bottom() <= height,
                "the button ends at {} in a window {height} tall",
                button.bottom()
            );
            assert!(button.top() >= 0.0);
        }
    }

    #[test]
    fn a_short_page_keeps_the_button_just_below_it() {
        let mut h = Harness::new().sized(1000.0, 900.0);
        show(&mut h, 3);
        let last = h.at("Line 2", 0).unwrap();
        let button = h.at("Continue", 0).unwrap();
        assert!(button.top() > last.bottom());
        assert!(button.top() - last.bottom() < 120.0);
        assert!(button.bottom() <= 900.0);
    }

    #[test]
    fn a_tall_page_shows_its_start_and_clips_the_rest_behind_a_scroll_area() {
        let mut h = Harness::new().sized(1000.0, 600.0);
        show(&mut h, 60);
        assert!(h.has("Line 0"));
        assert!(h.has("Line 1"));
        assert!(!h.has("Line 59"));
        assert!(h.at("Continue", 0).unwrap().bottom() <= 600.0);
    }

    #[test]
    fn the_steps_still_to_come_stand_out_from_the_background() {
        for p in [theme::LIGHT, theme::DARK] {
            let (_, color) = dot(&p, 4, 1);
            let gap = color
                .to_array()
                .iter()
                .zip(p.bg.to_array())
                .take(3)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap_or(0);
            assert!(
                gap >= 24,
                "a later step is only {gap} away from the background"
            );
        }
    }
}
