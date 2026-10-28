use crate::app::{ScApp, Screen, ToastKind};
use crate::theme::{self, pal_ui};
use crate::{config_io, widgets};
use egui::{pos2, vec2, Align, Color32, FontId, Layout, RichText, Sense, Ui};

const STEPS: usize = 5;

#[derive(Default)]
pub struct WizardState {
    pub step: usize,
}

pub fn view(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    app.devices.ensure(&app.link);

    let avail = ui.available_size();
    let w = 540.0_f32.min(avail.x - 48.0);
    ui.vertical_centered(|ui| {
        ui.add_space(((avail.y - 520.0) / 2.0).clamp(18.0, 80.0));
        ui.allocate_ui_with_layout(vec2(w, 520.0), Layout::top_down(Align::Min), |ui| {
            progress(ui, app.wizard.step);
            ui.add_space(26.0);
            match app.wizard.step {
                0 => welcome(ui),
                1 => look(app, ui),
                2 => recording(app, ui),
                3 => shortcut(app, ui),
                _ => ready(app, ui),
            }
            ui.add_space(26.0);
            buttons(app, ui);
        });
    });
    let _ = p;
}

fn progress(ui: &mut Ui, step: usize) {
    let p = pal_ui(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for i in 0..STEPS {
            let (w, color) = if i == step {
                (26.0, p.accent)
            } else if i < step {
                (8.0, p.accent.linear_multiply(0.5))
            } else {
                (8.0, p.card_hi)
            };
            let (rect, _) = ui.allocate_exact_size(vec2(w, 8.0), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, color);
        }
    });
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
            s.row("Display", None, |ui| {
                let label = app
                    .devices
                    .monitors
                    .iter()
                    .find(|m| m.id == app.cfg.capture.monitor_id)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "Main display".to_string());
                egui::ComboBox::from_id_salt("wiz-monitor")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.cfg.capture.monitor_id,
                            String::new(),
                            "Main display",
                        );
                        for m in &app.devices.monitors {
                            ui.selectable_value(
                                &mut app.cfg.capture.monitor_id,
                                m.id.clone(),
                                m.name.clone(),
                            );
                        }
                    });
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
