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
    let top = ui.cursor().top();
    let paused = s.state == CaptureState::Paused;
    let secs = s.buffer_secs;

    widgets::page_title(ui, if paused { "Paused" } else { "You’re covered" }, None);
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
    hints(app, ui);
    #[cfg(windows)]
    addon_buttons(app, ui);
    let used = ui.cursor().top() - top;
    ui.add_space(fold_gap(ui.clip_rect().height(), used));
    insights(app, ui);
}

pub const PEEK: f32 = 84.0;

pub fn fold_gap(view: f32, used: f32) -> f32 {
    (view - 20.0 - used - PEEK).max(36.0)
}

fn insights(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let st = crate::screens::gallery::stats(app);
    let head = ui.horizontal(|ui| {
        ui.label(
            RichText::new("Your stats")
                .font(FontId::new(20.0, theme::semibold()))
                .color(p.text),
        );
        if let Some(d) = st.since {
            ui.label(
                RichText::new(format!("since {}", crate::stats::date_label(d)))
                    .size(13.0)
                    .color(p.text_3),
            );
        }
    });
    let low = ui.clip_rect().bottom() - head.response.rect.top() < PEEK + 40.0;
    let a = ui
        .ctx()
        .animate_bool_with_time(ui.id().with("stats-peek"), low, 0.25);
    if a > 0.01 {
        ui.painter().text(
            pos2(ui.max_rect().right(), head.response.rect.center().y),
            Align2::RIGHT_CENTER,
            "Scroll down for more",
            FontId::new(12.5, egui::FontFamily::Proportional),
            p.accent.linear_multiply(a),
        );
    }
    ui.add_space(8.0);
    if st.clips + st.shots + st.edited + st.compressed == 0 {
        widgets::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                widgets::secondary(ui, "Your stats fill in as you save clips.");
                ui.add_space(10.0);
            });
        });
        return;
    }
    let size = crate::screens::gallery::size_label;
    let gap = 12.0;
    let w = ((ui.available_width() - gap * 3.0) / 4.0).floor();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let row = |ui: &mut Ui, tiles: [(&str, String, String); 4]| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (l, v, sub) in tiles {
                big_tile(ui, w, l, &v, &sub);
            }
        });
    };
    row(
        ui,
        [
            (
                "Clips saved",
                st.clips.to_string(),
                format!("{} today", st.today),
            ),
            (
                "Screenshots",
                st.shots.to_string(),
                "PNG files in your folder".into(),
            ),
            (
                "Space used",
                size(st.bytes),
                format!("about {} a clip", size(st.avg_bytes())),
            ),
            (
                "Day streak",
                plural(st.streak as usize, "day", "days"),
                format!("best {}", plural(st.best_streak as usize, "day", "days")),
            ),
        ],
    );
    ui.add_space(gap);
    let half = ((ui.available_width() - gap) / 2.0).floor();
    let when = st
        .hour
        .map(crate::stats::hour_label)
        .and_then(|l| {
            l.split_once(", ")
                .map(|(a, b)| (a.to_string(), b.to_string()))
        })
        .unwrap_or_else(|| ("\u{2013}".into(), String::new()));
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        panel(ui, half, "Last 14 days", |ui| days_chart(ui, &st.days));
        panel(ui, half, "Top games", |ui| games_chart(ui, &st.games));
    });
    ui.add_space(gap);
    row(
        ui,
        [
            (
                "This week",
                st.week.to_string(),
                "clips in the last 7 days".into(),
            ),
            (
                "Busiest day",
                st.busiest
                    .map(|b| plural(b.1, "clip", "clips"))
                    .unwrap_or_else(|| "\u{2013}".into()),
                st.busiest
                    .map(|b| crate::stats::short_date(b.0))
                    .unwrap_or_default(),
            ),
            ("You usually clip", when.0, when.1),
            (
                "Edited",
                (st.edited + st.compressed).to_string(),
                format!("{} trimmed, {} compressed", st.edited, st.compressed),
            ),
        ],
    );
    ui.add_space(gap);
    let games = st.games.iter().filter(|g| g.0 != "Everything else").count();
    widgets::card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let bits = [
                plural(st.clips + st.edited + st.compressed, "video", "videos"),
                plural(games, "game", "games"),
                plural(st.marked, "bookmarked clip", "bookmarked clips"),
                format!("{} on disk", size(st.bytes)),
            ];
            for (i, b) in bits.iter().enumerate() {
                if i > 0 {
                    ui.label(RichText::new("\u{00B7}").color(p.text_3));
                }
                ui.label(RichText::new(b).size(13.0).color(p.text_2));
            }
        });
    });
}

fn big_tile(ui: &mut Ui, w: f32, label: &str, value: &str, sub: &str) {
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
                ui.label(
                    RichText::new(label.to_uppercase())
                        .font(FontId::new(10.5, theme::semibold()))
                        .color(p.text_3),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(value)
                            .font(FontId::new(22.0, theme::semibold()))
                            .color(p.text),
                    )
                    .truncate(),
                );
                ui.add(egui::Label::new(RichText::new(sub).size(12.0).color(p.text_2)).truncate());
            });
        });
}

fn panel(ui: &mut Ui, w: f32, title: &str, body: impl FnOnce(&mut Ui)) {
    let p = pal_ui(ui);
    egui::Frame::none()
        .fill(p.card)
        .rounding(egui::Rounding::same(theme::radius::TILE))
        .stroke(Stroke::new(1.0_f32, p.line))
        .inner_margin(egui::Margin::symmetric(16.0, 14.0))
        .show(ui, |ui| {
            ui.set_width(w - 32.0 - 2.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .font(FontId::new(10.5, theme::semibold()))
                        .color(p.text_3),
                );
                ui.add_space(10.0);
                body(ui);
            });
        });
}

fn days_chart(ui: &mut Ui, days: &[(time::Date, usize)]) {
    let p = pal_ui(ui);
    let h = 120.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let base = rect.bottom() - 18.0;
    let max = days.iter().map(|d| d.1).max().unwrap_or(0).max(1) as f32;
    let n = days.len().max(1) as f32;
    let slot = rect.width() / n;
    let bw = (slot - 2.0).clamp(4.0, 22.0);
    ui.painter().line_segment(
        [pos2(rect.left(), base), pos2(rect.right(), base)],
        Stroke::new(1.0_f32, p.line),
    );
    for (i, (d, c)) in days.iter().enumerate() {
        let cx = rect.left() + slot * (i as f32 + 0.5);
        let col = egui::Rect::from_min_max(
            pos2(rect.left() + slot * i as f32, rect.top()),
            pos2(rect.left() + slot * (i as f32 + 1.0), base),
        );
        let resp = ui.interact(col, ui.id().with(("stats-day", i)), Sense::hover());
        let hot = resp.hovered();
        if *c > 0 {
            let bh = ((base - rect.top() - 14.0) * (*c as f32 / max)).max(3.0);
            let r =
                egui::Rect::from_min_max(pos2(cx - bw / 2.0, base - bh), pos2(cx + bw / 2.0, base));
            let fill = if hot {
                p.accent
            } else {
                p.accent.linear_multiply(0.8)
            };
            ui.painter().rect_filled(
                r,
                egui::Rounding {
                    nw: 4.0,
                    ne: 4.0,
                    sw: 0.0,
                    se: 0.0,
                },
                fill,
            );
        }
        if i == 0 || i + 1 == days.len() {
            ui.painter().text(
                pos2(cx, base + 10.0),
                Align2::CENTER_CENTER,
                if i + 1 == days.len() {
                    "Today".to_string()
                } else {
                    format!("{}", d.day())
                },
                FontId::new(11.0, egui::FontFamily::Proportional),
                p.text_3,
            );
        }
        if hot {
            ui.painter()
                .rect_filled(col, 4.0, p.text_3.linear_multiply(0.08));
            resp.on_hover_text(format!(
                "{} \u{00B7} {}",
                crate::stats::short_date(*d),
                if *c == 1 {
                    "1 clip".to_string()
                } else {
                    format!("{c} clips")
                }
            ));
        }
    }
}

fn games_chart(ui: &mut Ui, games: &[(String, usize)]) {
    let p = pal_ui(ui);
    let max = games.iter().map(|g| g.1).max().unwrap_or(0).max(1) as f32;
    let rows = 5.0;
    let lh = 120.0 / rows;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    if games.is_empty() {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No clips yet",
            FontId::new(13.0, egui::FontFamily::Proportional),
            p.text_3,
        );
        return;
    }
    let name_w = (rect.width() * 0.34).min(120.0);
    let count_w = 34.0;
    let track = rect.width() - name_w - count_w - 8.0;
    for (i, (g, n)) in games.iter().enumerate() {
        let y = rect.top() + lh * (i as f32 + 0.5);
        let name = ui.painter().layout_no_wrap(
            g.clone(),
            FontId::new(12.5, egui::FontFamily::Proportional),
            p.text_2,
        );
        let shown = if name.size().x > name_w - 6.0 {
            let mut s: String = g.chars().take(10).collect();
            s.push('\u{2026}');
            s
        } else {
            g.clone()
        };
        ui.painter().text(
            pos2(rect.left(), y),
            Align2::LEFT_CENTER,
            shown,
            FontId::new(12.5, egui::FontFamily::Proportional),
            p.text_2,
        );
        let x0 = rect.left() + name_w;
        let bw = (track * (*n as f32 / max)).max(4.0);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(pos2(x0, y - 6.0), pos2(x0 + bw, y + 6.0)),
            egui::Rounding {
                nw: 0.0,
                sw: 0.0,
                ne: 4.0,
                se: 4.0,
            },
            p.accent.linear_multiply(0.8),
        );
        ui.painter().text(
            pos2(x0 + bw + 8.0, y),
            Align2::LEFT_CENTER,
            n.to_string(),
            FontId::new(12.5, theme::semibold()),
            p.text,
        );
    }
}

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

fn ring(ui: &mut Ui, fill: f32, secs: u32, paused: bool) {
    let p = pal_ui(ui);
    let size = 148.0;
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let c = rect.center();
    let r = size / 2.0 - 16.0;
    let w = 10.0;
    let lit =
        ui.ctx()
            .animate_value_with_time(ui.id().with("ring-ticks"), fill.clamp(0.0, 1.0), 0.4);
    for i in 0..60 {
        let a = std::f32::consts::TAU * (i as f32 / 60.0) - std::f32::consts::FRAC_PI_2;
        let long = i % 5 == 0;
        let (r0, r1) = (
            r + w / 2.0 + 5.0,
            r + w / 2.0 + if long { 10.0 } else { 8.0 },
        );
        let on = (i as f32 / 60.0) < lit;
        ui.painter().line_segment(
            [
                pos2(c.x + r0 * a.cos(), c.y + r0 * a.sin()),
                pos2(c.x + r1 * a.cos(), c.y + r1 * a.sin()),
            ],
            Stroke::new(
                1.4_f32,
                if on {
                    p.text_3
                } else {
                    p.text_3.linear_multiply(0.35)
                },
            ),
        );
    }
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
                ui.label(
                    RichText::new(label.to_uppercase())
                        .font(FontId::new(10.5, theme::semibold()))
                        .color(p.text_3),
                );
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
                    crate::screens::gallery::play(app, &clip.path);
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
            app.gallery.rescan();
        }
        Ok(Response::Error { message }) => app.toast(message, ToastKind::Warn),
        Ok(_) => {}
        Err(e) => app.toast(e.to_string(), ToastKind::Error),
    }
}

#[cfg(windows)]
fn addon_buttons(app: &mut ScApp, ui: &mut Ui) {
    use crate::addon::Reply;
    app.settings.addon.refresh();
    match app.settings.addon.answer() {
        Some(Reply::Open(path)) if path.is_file() => crate::screens::gallery::play(app, &path),
        Some(Reply::Open(_)) => app.toast("That clip is no longer there", ToastKind::Warn),
        Some(Reply::Toast(t)) => app.toast(t, ToastKind::Ok),
        Some(Reply::Error(e)) => app.toast(e, ToastKind::Warn),
        Some(Reply::Quiet) | None => {}
    }
    let list = app.settings.addon.home_buttons();
    if list.is_empty() {
        return;
    }
    let busy = app.settings.addon.pressing();
    let mut hit = None;
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        ui.add_enabled_ui(!busy, |ui| {
            for (i, h) in list.iter().enumerate() {
                let glyph = crate::addon::glyph(&h.button.icon);
                let mut r = widgets::quiet_button(ui, Some(glyph), &h.button.label);
                if !h.button.hint.is_empty() {
                    r = r.on_hover_text(&h.button.hint);
                }
                if r.clicked() {
                    hit = Some(i);
                }
            }
        });
    });
    if let Some(h) = hit.and_then(|i| list.get(i)) {
        let clips = app.cfg.save.resolve_dir();
        app.settings.addon.press(h, clips, ui.ctx());
    }
}

fn hints(app: &ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    ui.add_space(24.0);
    egui::Frame::none()
        .fill(p.card)
        .rounding(egui::Rounding::same(theme::radius::TILE))
        .stroke(Stroke::new(1.0_f32, p.line))
        .inner_margin(egui::Margin::symmetric(18.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let keys = [
                    ("Save clip", &app.cfg.hotkeys.save),
                    ("Screenshot", &app.cfg.hotkeys.screenshot),
                    ("Open SimpleClip", &app.cfg.hotkeys.open),
                ];
                for (label, combo) in keys {
                    if combo.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(label).size(13.0).color(p.text_2));
                    widgets::keycaps(ui, combo);
                    ui.add_space(16.0);
                }
            });
        });
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    #[test]
    fn stats_peek_above_the_bottom_edge() {
        let (view, used) = (800.0, 520.0);
        let top = 20.0 + used + fold_gap(view, used);
        assert_eq!(view - top, PEEK);
    }

    #[test]
    fn short_windows_keep_a_normal_gap() {
        assert_eq!(fold_gap(470.0, 600.0), 36.0);
    }
}
