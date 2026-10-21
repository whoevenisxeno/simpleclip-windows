use crate::app::{ScApp, Screen, ToastKind};
use crate::daemon::Link;
use crate::theme::{self, icon, pal_ui};
use crate::widgets;
use egui::{pos2, vec2, Align, Align2, FontId, Layout, RichText, Sense, Shape, Stroke, Ui};
use sc_core::capture::CaptureState;
use sc_core::ipc::{Request, Response, StatusReport};

pub fn view(app: &mut ScApp, ui: &mut Ui) {
    match app.link.clone() {
        Link::Disconnected => offline(app, ui),
        Link::Connected(s) => online(app, ui, &s),
    }
}

fn offline(app: &mut ScApp, ui: &mut Ui) {
    let _ = app;
    let p = pal_ui(ui);
    widgets::page_title(ui, "Home", None);
    widgets::card(ui, |ui| {
        ui.add_space(18.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("SimpleClip isn’t running")
                    .font(FontId::new(20.0, theme::semibold()))
                    .color(p.text),
            );
            ui.add_space(4.0);
            widgets::secondary(
                ui,
                "The background service keeps your replay buffer. Start it to begin.",
            );
            ui.add_space(14.0);
            if widgets::primary_button(ui, Some(icon::PLAY), "Start SimpleClip").clicked() {
                crate::daemon::spawn_daemon();
            }
        });
        ui.add_space(18.0);
    });
}

fn online(app: &mut ScApp, ui: &mut Ui, s: &StatusReport) {
    let p = pal_ui(ui);
    let paused = s.state == CaptureState::Paused;
    let secs = s.buffer_secs;

    widgets::page_title(ui, if paused { "Paused" } else { "You’re covered" }, None);
    // Subtitle with the hotkey drawn as key caps.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(if paused {
                "Buffering is paused. Resume to keep recording."
            } else {
                "The last"
            })
            .color(p.text_2)
            .size(14.0),
        );
        if !paused {
            ui.label(
                RichText::new(format!("{secs} seconds"))
                    .color(p.text)
                    .size(14.0),
            );
            ui.label(
                RichText::new("are always ready. Press")
                    .color(p.text_2)
                    .size(14.0),
            );
            widgets::keycaps(ui, &app.cfg.hotkeys.save);
            ui.label(RichText::new("to keep them.").color(p.text_2).size(14.0));
        }
    });
    ui.add_space(16.0);

    hero(app, ui, s, paused);
    ui.add_space(14.0);
    stats(app, ui, s);
    ui.add_space(18.0);
    recent(app, ui);
}

/// Buffer ring on the left, the main actions on the right.
fn hero(app: &mut ScApp, ui: &mut Ui, s: &StatusReport, paused: bool) {
    let p = pal_ui(ui);
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ring(ui, s.buffer_fill, s.buffer_secs, paused);
            ui.add_space(22.0);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                ui.label(
                    RichText::new("Save what just happened")
                        .font(FontId::new(20.0, theme::semibold()))
                        .color(p.text),
                );
                ui.add_space(2.0);
                widgets::secondary(
                    ui,
                    format!(
                        "Saves the last {}s as an MP4 in your clips folder.",
                        s.buffer_secs
                    ),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if widgets::primary_button(ui, Some(icon::SAVE), "Save clip").clicked() {
                        send(app, Request::Save { last_secs: None });
                    }
                    if widgets::quiet_button(ui, Some(icon::CAMERA), "Screenshot").clicked() {
                        send(app, Request::Screenshot);
                    }
                    let (g, l, r) = if paused {
                        (icon::PLAY, "Resume", Request::Resume)
                    } else {
                        (icon::PAUSE, "Pause", Request::Pause)
                    };
                    if widgets::quiet_button(ui, Some(g), l).clicked() {
                        send(app, r);
                    }
                });
            });
        });
    });
}

/// A circular gauge of how full the replay buffer is.
fn ring(ui: &mut Ui, fill: f32, secs: u32, paused: bool) {
    let p = pal_ui(ui);
    let size = 132.0;
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let c = rect.center();
    let r = size / 2.0 - 8.0;
    let w = 9.0;
    ui.painter().circle_stroke(c, r, Stroke::new(w, p.card_hi));
    let color = if paused { p.orange } else { p.accent };
    let t = ui
        .ctx()
        .animate_value_with_time(ui.id().with("ring"), fill.clamp(0.0, 1.0), 0.4);
    if t > 0.003 {
        let n = ((t * 120.0) as usize).max(2);
        let start = -std::f32::consts::FRAC_PI_2;
        let pts: Vec<_> = (0..=n)
            .map(|i| {
                let a = start + std::f32::consts::TAU * t * (i as f32 / n as f32);
                pos2(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect();
        ui.painter().add(Shape::line(pts, Stroke::new(w, color)));
        // Rounded caps.
        for a in [start, start + std::f32::consts::TAU * t] {
            ui.painter()
                .circle_filled(pos2(c.x + r * a.cos(), c.y + r * a.sin()), w / 2.0, color);
        }
    }
    ui.painter().text(
        pos2(c.x, c.y - 7.0),
        Align2::CENTER_CENTER,
        format!("{secs}s"),
        FontId::new(30.0, theme::semibold()),
        p.text,
    );
    ui.painter().text(
        pos2(c.x, c.y + 18.0),
        Align2::CENTER_CENTER,
        format!("{:.0}% buffered", fill * 100.0),
        FontId::new(12.0, egui::FontFamily::Proportional),
        p.text_2,
    );
}

fn stats(app: &ScApp, ui: &mut Ui, s: &StatusReport) {
    let enc = s
        .encoder
        .map(|e| match format!("{e:?}").as_str() {
            "Amf" => "AMD AMF".to_string(),
            "Nvenc" => "NVIDIA NVENC".to_string(),
            "Qsv" => "Intel QuickSync".to_string(),
            "Vaapi" => "VA-API".to_string(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "\u{2013}".into());
    let display = s
        .monitor
        .as_ref()
        .map(|m| {
            format!(
                "{}\u{00D7}{} \u{00B7} {} fps",
                m.width, m.height, app.cfg.capture.target_fps
            )
        })
        .unwrap_or_else(|| "\u{2013}".into());
    let audio = match (
        app.cfg.audio.desktop_enabled,
        cfg!(windows) && app.cfg.audio.mic_enabled,
    ) {
        (true, true) => "System + mic",
        (true, false) => "System sound",
        (false, true) => "Microphone",
        (false, false) => "Off",
    };
    let tiles = [
        ("Encoder", enc),
        ("Display", display),
        ("Audio", audio.to_string()),
        (
            "Quality",
            format!("{} Mbps", app.cfg.encode.bitrate_kbps / 1000),
        ),
    ];
    let gap = 12.0;
    let w = ((ui.available_width() - gap * 3.0) / 4.0).floor();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (label, value) in tiles {
            tile(ui, w, label, &value);
        }
    });
}

fn tile(ui: &mut Ui, w: f32, label: &str, value: &str) {
    let p = pal_ui(ui);
    egui::Frame::none()
        .fill(p.card)
        .rounding(egui::Rounding::same(theme::radius::TILE))
        .stroke(Stroke::new(1.0_f32, p.line))
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(w - 28.0 - 2.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(label).size(12.0).color(p.text_2));
                ui.add(
                    egui::Label::new(
                        RichText::new(value)
                            .font(FontId::new(15.0, theme::semibold()))
                            .color(p.text),
                    )
                    .truncate(),
                );
            });
        });
}

/// The latest few clips as a row of previews.
fn recent(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let clips = crate::screens::gallery::recent_clips(app, 4);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Recent clips")
                .font(FontId::new(20.0, theme::semibold()))
                .color(p.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(
                    egui::Label::new(RichText::new("See all").color(p.accent))
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                app.goto(Screen::Gallery);
            }
        });
    });
    ui.add_space(6.0);
    if clips.is_empty() {
        widgets::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                widgets::secondary(ui, "Nothing saved yet. Your first clip will show up here.");
                ui.add_space(10.0);
            });
        });
        return;
    }
    let gap = 12.0;
    let w = ((ui.available_width() - gap * 3.0) / 4.0).floor();
    let h = (w * 9.0 / 16.0).round();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for clip in clips {
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing.y = 6.0;
                let resp = crate::screens::gallery::preview(app, ui, &clip, vec2(w, h));
                if resp.clicked() {
                    crate::screens::gallery::open_path(&clip.path);
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::screens::gallery::recent_title(&clip))
                            .font(FontId::new(13.0, theme::semibold()))
                            .color(p.text),
                    )
                    .truncate(),
                );
                ui.label(
                    RichText::new(crate::screens::gallery::ago(clip.modified))
                        .size(12.0)
                        .color(p.text_2),
                );
            });
        }
    });
}

fn send(app: &mut ScApp, req: Request) {
    match crate::daemon::request(req) {
        Ok(Response::Saved { .. }) => {
            // The desktop popup already announced it; the gallery just needs a refresh.
            app.gallery.rescan();
        }
        Ok(Response::Error { message }) => app.toast(message, ToastKind::Warn),
        Ok(_) => {}
        Err(e) => app.toast(e.to_string(), ToastKind::Error),
    }
}
