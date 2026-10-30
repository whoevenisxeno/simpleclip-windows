use crate::app::{ScApp, ToastKind};
use crate::screens::gallery;
use crate::theme::{self, pal_ui};
use crate::widgets::{self, section};
use egui::{vec2, Align, Layout, RichText, Sense, Ui};
use sc_core::config::{AudioTracks, FolderPolicy, PopupPosition, PopupStyle, MAX_VOLUME_PERCENT};
use std::time::Duration;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Capture,
    Sound,
    Quality,
    Storage,
    Hotkeys,
    SaveCard,
    Look,
    System,
}

const TABS: [(Tab, &str, &str); 8] = [
    (
        Tab::Capture,
        "Capture",
        "What gets recorded into the buffer.",
    ),
    (
        Tab::Sound,
        "Sound",
        "Game and app audio, plus an optional microphone.",
    ),
    (
        Tab::Quality,
        "Quality",
        "Sharper looks better and takes more space.",
    ),
    (Tab::Storage, "Storage", "Where clips and trims are kept."),
    (
        Tab::Hotkeys,
        "Hotkeys",
        "Click Change, then press the keys. Any key works, the backtick included.",
    ),
    (
        Tab::SaveCard,
        "Save card",
        "The small card that appears when a clip is saved.",
    ),
    (
        Tab::Look,
        "Look",
        "Pick a theme and an accent colour. The default follows Windows light and dark.",
    ),
    (
        Tab::System,
        "System",
        "Startup, sharing, updates and housekeeping.",
    ),
];

#[derive(Default)]
pub struct SettingsState {
    pub tab: Tab,
    pub query: String,
    pub dupes: Option<crate::dupes::Plan>,
}

pub fn view(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    app.devices.ensure(&app.link);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            widgets::page_title(ui, "Settings", Some("Changes are saved automatically."));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(4.0);
            status_pill(app, ui);
            ui.add_space(6.0);
            search_box(ui, &mut app.settings.query);
        });
    });
    ui.add_space(4.0);

    widgets::set_search_query(ui.ctx(), &app.settings.query);
    widgets::reset_rows(ui.ctx());
    let searching = !widgets::search_query(ui.ctx()).is_empty();

    let height = (ui.available_height() - 12.0).max(200.0);
    if ui.available_width() < 700.0 {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for (tab, label, _) in TABS {
                let selected = !searching && app.settings.tab == tab;
                if tab_chip(ui, label, selected).clicked() {
                    app.settings.tab = tab;
                    app.settings.query.clear();
                }
            }
            if tab_chip(ui, "Setup guide", false).clicked() {
                app.wizard = Default::default();
                app.goto(crate::app::Screen::Wizard);
            }
        });
        ui.add_space(8.0);
        let pane_height = (ui.available_height() - 12.0).max(160.0);
        pane_scroll(app, ui, pane_height, searching);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        ui.allocate_ui_with_layout(vec2(168.0, height), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            for (tab, label, _) in TABS {
                let selected = !searching && app.settings.tab == tab;
                if rail_item(ui, label, selected, tab_icon(tab)).clicked() {
                    app.settings.dupes = None;
                    app.settings.tab = tab;
                    app.settings.query.clear();
                }
            }
            ui.add_space(10.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, p.line);
            ui.add_space(10.0);
            if rail_item(ui, "Setup guide", false, Icon::Star).clicked() {
                app.wizard = Default::default();
                app.goto(crate::app::Screen::Wizard);
            }
        });
        ui.vertical(|ui| pane_scroll(app, ui, height, searching));
    });
}

fn pane_scroll(app: &mut ScApp, ui: &mut Ui, height: f32, searching: bool) {
    let p = pal_ui(ui);
    egui::ScrollArea::vertical()
        .id_salt("settings-pane")
        .auto_shrink([false, false])
        .max_height(height)
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - 8.0);
            if searching {
                results(app, ui);
            } else {
                let (tab, title, sub) = TABS
                    .iter()
                    .find(|(t, _, _)| *t == app.settings.tab)
                    .copied()
                    .unwrap_or(TABS[0]);
                ui.label(
                    RichText::new(title)
                        .font(egui::FontId::new(22.0, theme::semibold()))
                        .color(p.text),
                );
                ui.add_space(-4.0);
                ui.label(RichText::new(sub).size(13.5).color(p.text_2));
                ui.add_space(14.0);
                pane(app, ui, tab);
                ui.add_space(24.0);
            }
        });
}

fn tab_chip(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let p = pal_ui(ui);
    let font = egui::FontId::new(
        13.5,
        if selected {
            theme::semibold()
        } else {
            egui::FontFamily::Proportional
        },
    );
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, p.text);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(24.0, 14.0), Sense::click());
    let round = egui::Rounding::same(rect.height() / 2.0);
    if selected {
        ui.painter().rect_filled(rect, round, p.card);
        ui.painter()
            .rect_stroke(rect, round, egui::Stroke::new(1.0_f32, p.line));
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, round, p.text.linear_multiply(0.06));
    }
    ui.painter().galley_with_override_text_color(
        rect.center() - galley.size() / 2.0,
        galley,
        if selected { p.text } else { p.text_2 },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn pane(app: &mut ScApp, ui: &mut Ui, tab: Tab) {
    match tab {
        Tab::Capture => {
            presets_section(app, ui);
            recording(app, ui);
        }
        Tab::Sound => audio(app, ui),
        Tab::Quality => quality(app, ui),
        Tab::Storage => {
            saving(app, ui);
            cleanup(app, ui);
        }
        Tab::Hotkeys => shortcuts(app, ui),
        Tab::SaveCard => notifications(app, ui),
        Tab::Look => appearance(app, ui),
        Tab::System => {
            general(app, ui);
            advanced(app, ui);
            reset_section(app, ui);
            #[cfg(windows)]
            uninstall_section(app, ui);
        }
    }
}

fn results(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let frame = egui::Frame::none()
        .fill(p.card)
        .rounding(egui::Rounding::same(theme::radius::CARD))
        .stroke(egui::Stroke::new(1.0_f32, p.line))
        .shadow(theme::card_shadow(&p))
        .inner_margin(egui::Margin::symmetric(16.0, 0.0));
    let mut prepared = frame.begin(ui);
    {
        let inner = &mut prepared.content_ui;
        inner.set_width(inner.available_width());
        inner.spacing_mut().item_spacing.y = 0.0;
        for (tab, _, _) in TABS {
            pane(app, inner, tab);
        }
    }
    if widgets::rows_drawn(ui.ctx()) > 0 {
        prepared.end(ui);
    } else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Nothing matches that search.")
                    .size(14.0)
                    .color(p.text_2),
            );
        });
    }
}

fn status_pill(app: &ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let (color, text) = if let Some(err) = &app.autosave.error {
        (p.red, format!("Not saved: {err}"))
    } else if app.settings_dirty() {
        (p.orange, "Saving\u{2026}".to_string())
    } else {
        (p.green, "All changes saved".to_string())
    };
    egui::Frame::none()
        .fill(color.linear_multiply(0.14))
        .rounding(egui::Rounding::same(14.0))
        .inner_margin(egui::Margin::symmetric(12.0, 6.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                widgets::dot(ui, color);
                ui.label(RichText::new(text).size(12.5).color(color));
            });
        });
}

fn search_box(ui: &mut Ui, query: &mut String) {
    let p = pal_ui(ui);
    egui::Frame::none()
        .fill(p.card_hi)
        .rounding(egui::Rounding::same(theme::radius::CONTROL))
        .inner_margin(egui::Margin::symmetric(11.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if theme::has_icons(ui.ctx()) {
                    ui.label(
                        RichText::new(theme::icon::SEARCH)
                            .font(egui::FontId::new(14.0, theme::icons()))
                            .color(p.text_3),
                    );
                }
                ui.add(
                    egui::TextEdit::singleline(query)
                        .hint_text("Search settings")
                        .frame(false)
                        .desired_width(170.0),
                );
            });
        });
}

fn rail_item(ui: &mut Ui, label: &str, selected: bool, icon: Icon) -> egui::Response {
    let p = pal_ui(ui);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let on = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("on"), selected, 0.16);
    let round = egui::Rounding::same(theme::radius::CONTROL - 1.0);
    if on < 1.0 {
        ui.painter().rect_filled(
            rect,
            round,
            p.text.linear_multiply(0.06 * hover * (1.0 - on)),
        );
    }
    if on > 0.0 {
        ui.painter()
            .rect_filled(rect, round, theme::mix(p.bg, p.card, on));
        ui.painter().rect_stroke(
            rect,
            round,
            egui::Stroke::new(1.0_f32, p.line.linear_multiply(on)),
        );
        ui.painter().rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 3.0, rect.center().y),
                vec2(3.0, 16.0 * on),
            ),
            egui::Rounding::same(1.5),
            p.accent,
        );
    }
    paint_icon(
        ui.painter(),
        egui::pos2(rect.left() + 25.0, rect.center().y),
        icon,
        if selected { p.accent } else { p.text_2 },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 45.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(
            14.0,
            if selected {
                theme::semibold()
            } else {
                egui::FontFamily::Proportional
            },
        ),
        if selected { p.text } else { p.text_2 },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn choice(ui: &mut Ui, value: &mut u32, options: &[(u32, &str)], suffix: &str) {
    let mut opts: Vec<(u32, String)> = options.iter().map(|(v, l)| (*v, l.to_string())).collect();
    if !opts.iter().any(|(v, _)| v == value) {
        opts.push((*value, format!("{value}{suffix}")));
        opts.sort_by_key(|(v, _)| *v);
    }
    let refs: Vec<(u32, &str)> = opts.iter().map(|(v, l)| (*v, l.as_str())).collect();
    widgets::segmented(ui, value, &refs);
}

fn recording(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    section(ui, Some("Recording"), |s| {
        if cfg!(windows) && !app.devices.monitors.is_empty() {
            s.full("display monitor screen record preview", |ui| {
                ui.label(RichText::new("Display").size(14.0).color(p.text));
                ui.label(
                    RichText::new("Which screen to record. The pictures show what each screen looks like right now.")
                        .size(12.0)
                        .color(p.text_2),
                );
                ui.add_space(10.0);
                widgets::monitor_picker(ui, &mut app.devices, &mut app.cfg.capture.monitor_id);
            });
        } else if cfg!(windows) {
            s.row("Display", Some("Which screen to record"), |ui| {
                let current = app.cfg.capture.monitor_id.clone();
                let label = app
                    .devices
                    .monitors
                    .iter()
                    .find(|m| m.id == current)
                    .map(monitor_label)
                    .unwrap_or_else(|| "Main display".to_string());
                egui::ComboBox::from_id_salt("monitor")
                    .selected_text(label)
                    .width(230.0)
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
                                monitor_label(m),
                            );
                        }
                    });
            });
        }
        s.row(
            "Replay length",
            Some("How much is kept ready to save"),
            |ui| {
                choice(
                    ui,
                    &mut app.cfg.buffer.replay_duration_secs,
                    &[
                        (15, "15 s"),
                        (30, "30 s"),
                        (45, "45 s"),
                        (60, "1 min"),
                        (120, "2 min"),
                        (300, "5 min"),
                    ],
                    " s",
                );
            },
        );
        s.row(
            "Frame rate",
            Some("Higher is smoother but needs a stronger PC"),
            |ui| {
                let before = app.cfg.capture.target_fps;
                choice(
                    ui,
                    &mut app.cfg.capture.target_fps,
                    &[
                        (30, "30"),
                        (60, "60"),
                        (120, "120"),
                        (144, "144"),
                        (165, "165"),
                        (240, "240"),
                    ],
                    " fps",
                );
                let now = app.cfg.capture.target_fps;
                if now != before && before > 0 {
                    let g = &mut app.cfg.encode.gop_frames;
                    *g = ((*g as f32 * now as f32 / before as f32).round() as u32).max(1);
                }
            },
        );
        s.row("Show mouse cursor", None, |ui| {
            widgets::toggle(ui, &mut app.cfg.capture.show_cursor);
        });
    });
}

fn monitor_label(m: &sc_core::capture::MonitorInfo) -> String {
    let main = if m.primary { " \u{00B7} main" } else { "" };
    format!("{}  ({}\u{00D7}{}{main})", m.name, m.width, m.height)
}

fn audio(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.row("System sound", Some("Game and app audio"), |ui| {
            widgets::toggle(ui, &mut app.cfg.audio.desktop_enabled);
        });
        if cfg!(windows)
            && app.cfg.audio.desktop_enabled
            && app.devices.audio.iter().any(|d| d.is_monitor)
        {
            s.row("Output device", None, |ui| {
                device_combo(
                    ui,
                    "audio-out",
                    &mut app.cfg.audio.desktop_device,
                    &app.devices.audio,
                    true,
                );
            });
        }
        if app.cfg.audio.desktop_enabled {
            s.row(
                "System volume",
                Some("How loud game and app sound is in clips"),
                |ui| volume_slider(ui, &mut app.cfg.audio.desktop_volume),
            );
        }
        s.row("Microphone", Some("Mix your voice into clips"), |ui| {
            widgets::toggle(ui, &mut app.cfg.audio.mic_enabled);
        });
        if app.cfg.audio.mic_enabled {
            s.row(
                "Microphone volume",
                Some("How loud your voice is in clips"),
                |ui| volume_slider(ui, &mut app.cfg.audio.mic_volume),
            );
        }
        if cfg!(windows) && app.cfg.audio.mic_enabled {
            s.row("Input device", None, |ui| {
                let mut id = app.cfg.audio.mic_device.clone().unwrap_or_default();
                device_combo(ui, "mic", &mut id, &app.devices.audio, false);
                app.cfg.audio.mic_device = if id.is_empty() { None } else { Some(id) };
            });
        }
        if app.cfg.audio.desktop_enabled || app.cfg.audio.mic_enabled {
            s.row(
                "Sound timing",
                Some("Shift the sound if new clips sound out of sync"),
                |ui| sync_control(ui, &mut app.cfg.audio.sync_offset_ms),
            );
        }
        if app.cfg.audio.mic_enabled && app.cfg.audio.desktop_enabled {
            s.row(
                "Tracks",
                Some("Separate keeps your voice editable on its own"),
                |ui| {
                    widgets::segmented(
                        ui,
                        &mut app.cfg.audio.tracks,
                        &[
                            (AudioTracks::Mixed, "Mixed"),
                            (AudioTracks::Separate, "Separate"),
                        ],
                    );
                },
            );
        }
    });
    if cfg!(windows) && app.cfg.audio.mic_enabled && widgets::search_query(ui.ctx()).is_empty() {
        let p = pal_ui(ui);
        ui.label(
            RichText::new("If the microphone records silence, allow desktop apps in Windows Settings \u{203A} Privacy \u{203A} Microphone.")
                .size(12.0)
                .color(p.text_2),
        );
        ui.add_space(6.0);
    }
}

const SYNC_RANGE_MS: i32 = 1000;

fn sync_control(ui: &mut Ui, ms: &mut i32) {
    let steps = (SYNC_RANGE_MS / 10) as u32;
    widgets::value_label(
        ui,
        &if *ms == 0 {
            "In sync".to_string()
        } else {
            format!("{:+} ms", *ms)
        },
    );
    let mut v = (*ms / 10 + steps as i32).clamp(0, 2 * steps as i32) as u32;
    if widgets::step_slider(ui, &mut v, 2 * steps, steps).changed() {
        *ms = (v as i32 - steps as i32) * 10;
    }
}

fn volume_slider(ui: &mut Ui, percent: &mut u32) {
    let p = pal_ui(ui);
    widgets::percent_slider(ui, percent, MAX_VOLUME_PERCENT);
    ui.add_sized(
        [44.0, 20.0],
        egui::Label::new(
            RichText::new(format!("{percent}%"))
                .size(13.0)
                .color(p.text_2),
        ),
    );
}

fn device_combo(
    ui: &mut Ui,
    id: &str,
    value: &mut String,
    devices: &[sc_core::audio::AudioDevice],
    outputs: bool,
) {
    let default_label = if outputs {
        "Default output"
    } else {
        "Default microphone"
    };
    let label = devices
        .iter()
        .find(|d| d.is_monitor == outputs && d.id == *value)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| default_label.to_string());
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(230.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, String::new(), default_label);
            for d in devices.iter().filter(|d| d.is_monitor == outputs) {
                ui.selectable_value(value, d.id.clone(), d.name.clone());
            }
        });
}

fn quality(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.row(
            "Video quality",
            Some(&quality_hint(app.cfg.encode.bitrate_kbps)),
            |ui| {
                choice(
                    ui,
                    &mut app.cfg.encode.bitrate_kbps,
                    &[
                        (4_000, "Low"),
                        (8_000, "Standard"),
                        (20_000, "High"),
                        (40_000, "Ultra"),
                    ],
                    " kbps",
                );
            },
        );
        s.row(
            "Bitrate",
            Some("Fine tune it: higher is sharper and bigger"),
            |ui| {
                let mut mbps = app.cfg.encode.bitrate_kbps as f64 / 1000.0;
                if ui
                    .add(
                        egui::DragValue::new(&mut mbps)
                            .range(1.0..=100.0)
                            .speed(0.25)
                            .max_decimals(1)
                            .suffix(" Mbps"),
                    )
                    .changed()
                {
                    app.cfg.encode.bitrate_kbps = (mbps * 1000.0).round() as u32;
                }
            },
        );
        s.row(
            "Resolution",
            Some(if app.cfg.encode.max_height.is_some() {
                "Scaled down on the CPU, so 'Lower CPU use' is skipped"
            } else {
                "Native keeps your screen’s own size"
            }),
            |ui| {
                widgets::segmented(
                    ui,
                    &mut app.cfg.encode.max_height,
                    &[
                        (None, "Native"),
                        (Some(1440), "1440p"),
                        (Some(1080), "1080p"),
                        (Some(720), "720p"),
                        (Some(480), "480p"),
                    ],
                );
            },
        );
        if cfg!(windows) {
            s.row(
                "Lower CPU use",
                Some("Let the GPU convert colours before encoding"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.encode.gpu_convert);
                },
            );
        }
    });
}

fn quality_hint(kbps: u32) -> String {
    let mb_per_min = kbps as f32 / 8.0 * 60.0 / 1000.0;
    format!("About {mb_per_min:.0} MB per minute of video")
}

fn saving(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.row("Clips folder", Some("Where saved clips go"), |ui| {
            let dir = app.cfg.save.resolve_dir();
            if widgets::quiet_button(ui, Some(theme::icon::FOLDER), "Open").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                crate::screens::gallery::open_path(&dir);
            }
            if crate::dialogs::available()
                && widgets::quiet_button(ui, None, "Choose\u{2026}").clicked()
            {
                if let Some(d) = crate::dialogs::pick_folder(&dir) {
                    app.cfg.save.directory = Some(d);
                }
            }
            let p = pal_ui(ui);
            ui.add(
                egui::Label::new(
                    RichText::new(dir.display().to_string())
                        .size(13.0)
                        .color(p.text_2),
                )
                .truncate(),
            );
        });
        s.row(
            "Trimmed clips",
            Some("Where trimmed copies are saved"),
            |ui| {
                let set = app.cfg.save.trim_directory.clone();
                if set.is_some() && widgets::quiet_button(ui, None, "Reset").clicked() {
                    app.cfg.save.trim_directory = None;
                }
                if crate::dialogs::available()
                    && widgets::quiet_button(ui, None, "Choose\u{2026}").clicked()
                {
                    let start = set.clone().unwrap_or_else(|| app.cfg.save.resolve_dir());
                    if let Some(d) = crate::dialogs::pick_folder(&start) {
                        app.cfg.save.trim_directory = Some(d);
                    }
                }
                let p = pal_ui(ui);
                let label = match &set {
                    Some(d) => d.display().to_string(),
                    None => "Next to the original".to_string(),
                };
                ui.add(
                    egui::Label::new(RichText::new(label).size(13.0).color(p.text_2)).truncate(),
                );
            },
        );
        s.row("Organize into folders", None, |ui| {
            widgets::segmented(
                ui,
                &mut app.cfg.save.folder_policy,
                &[
                    (FolderPolicy::PerDay, "By day"),
                    (FolderPolicy::PerApp, "By game"),
                    (FolderPolicy::Flat, "One folder"),
                ],
            );
        });
    });
}

fn shortcuts(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.row("Save clip", Some("Works in games and full screen"), |ui| {
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.save, "hk-save");
        });
        s.row("Take screenshot", None, |ui| {
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.screenshot, "hk-shot");
        });
        if cfg!(windows) {
            s.row("Open SimpleClip", Some("Brings this window up"), |ui| {
                if !app.cfg.hotkeys.open.is_empty()
                    && widgets::quiet_button(ui, None, "Off").clicked()
                {
                    app.cfg.hotkeys.open.clear();
                }
                widgets::hotkey_field(ui, &mut app.cfg.hotkeys.open, "hk-open");
            });
        }
    });
}

fn general(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.row(
            "Ask about sharing after trimming",
            Some("Offers to copy the clip or compress it for Discord"),
            |ui| {
                widgets::toggle(ui, &mut app.cfg.general.share_prompt);
            },
        );
        if cfg!(windows) {
            s.row(
                "Discord status",
                Some("Shows only the game you’re playing, and for how long"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.general.discord_presence);
                },
            );
            s.row(
                "Open at login",
                Some("Runs quietly in the background so the buffer is always ready"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.general.autostart);
                },
            );
        }
    });
}

fn position_label(p: PopupPosition) -> &'static str {
    match p {
        PopupPosition::TopRight => "Top right",
        PopupPosition::TopLeft => "Top left",
        PopupPosition::BottomRight => "Bottom right",
        PopupPosition::BottomLeft => "Bottom left",
        PopupPosition::Center => "Center",
    }
}

fn notifications(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.row("Show a card when a clip is saved", None, |ui| {
            widgets::toggle(ui, &mut app.cfg.general.notify);
        });
        s.row(
            "Card style",
            Some("Minimal is one short line, much smaller"),
            |ui| {
                widgets::segmented(
                    ui,
                    &mut app.cfg.general.popup_style,
                    &[
                        (PopupStyle::Detailed, "Detailed"),
                        (PopupStyle::Minimal, "Minimal"),
                    ],
                );
            },
        );
        s.row(
            "Card position",
            Some("Where on the screen it appears"),
            |ui| {
                egui::ComboBox::from_id_salt("popup-position")
                    .selected_text(position_label(app.cfg.general.popup_position))
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for p in [
                            PopupPosition::TopRight,
                            PopupPosition::TopLeft,
                            PopupPosition::BottomRight,
                            PopupPosition::BottomLeft,
                            PopupPosition::Center,
                        ] {
                            ui.selectable_value(
                                &mut app.cfg.general.popup_position,
                                p,
                                position_label(p),
                            );
                        }
                    });
            },
        );
        s.row("Slide in and out", Some("Turn off to just fade"), |ui| {
            widgets::toggle(ui, &mut app.cfg.general.popup_animation);
        });
        s.row("Play a sound when a clip is saved", None, |ui| {
            widgets::toggle(ui, &mut app.cfg.general.save_sound);
        });
        if app.cfg.general.save_sound {
            let name = app
                .cfg
                .general
                .save_sound_file
                .as_ref()
                .and_then(|f| f.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Built-in camera shutter".to_string());
            s.row("Sound", Some(&name), |ui| {
                #[cfg(windows)]
                {
                    if app.cfg.general.save_sound_file.is_some()
                        && widgets::quiet_button(ui, None, "Use default").clicked()
                    {
                        app.cfg.general.save_sound_file = None;
                    }
                    if widgets::quiet_button(ui, None, "Choose file\u{2026}").clicked() {
                        if let Some(f) = rfd::FileDialog::new()
                            .set_title("Choose a sound for saved clips")
                            .add_filter("WAV audio", &["wav"])
                            .pick_file()
                        {
                            app.cfg.general.save_sound_file = Some(f);
                        }
                    }
                }
                #[cfg(not(windows))]
                let _ = ui;
            });
        }
        s.row(
            "Preview",
            Some("Shows a sample card and plays the sound"),
            |ui| {
                if widgets::quiet_button(ui, None, "Preview").clicked() {
                    app.flush_config();
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_millis(700));
                        let _ = crate::daemon::request(sc_core::ipc::Request::TestNotification);
                    });
                }
            },
        );
    });
}

fn advanced(app: &mut ScApp, ui: &mut Ui) {
    section(ui, Some("Advanced"), |s| {
        s.row("File name", Some("Use {app}, {date} and {time}"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.cfg.save.filename_template)
                    .desired_width(250.0),
            );
        });
        s.row(
            "Keyframe interval",
            Some("Time between full pictures; shorter makes cuts more exact"),
            |ui| {
                let fps = app.cfg.capture.target_fps.max(1);
                let mut secs = ((app.cfg.encode.gop_frames + fps / 2) / fps).max(1);
                let before = secs;
                choice(ui, &mut secs, &[(1, "1 s"), (2, "2 s"), (4, "4 s")], " s");
                if secs != before {
                    app.cfg.encode.gop_frames = secs * fps;
                }
            },
        );
        s.row(
            "Memory limit",
            Some("Most RAM the replay buffer may use"),
            |ui| {
                choice(
                    ui,
                    &mut app.cfg.buffer.ram_cap_mb,
                    &[
                        (512, "512 MB"),
                        (1024, "1 GB"),
                        (2048, "2 GB"),
                        (4096, "4 GB"),
                    ],
                    " MB",
                );
            },
        );
        #[cfg(windows)]
        {
            s.row(
                "Check for updates",
                Some("Looks on GitHub every 30 minutes for a newer version"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.general.update_check);
                },
            );
            s.row(
                "Sound when an update is found",
                Some("A short chime, different from the clip saved sound"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.general.update_sound);
                    if widgets::quiet_button(ui, None, "Preview").clicked() {
                        crate::sound::play_update();
                    }
                },
            );
            let status = app.updater.status_text();
            s.row("Version", Some(&status), |ui| {
                if widgets::quiet_button(ui, None, "Check now").clicked() {
                    app.updater.check(ui.ctx());
                }
            });
        }
    });
}

#[cfg(windows)]
fn uninstall_section(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let exe = crate::update::Updater::uninstall_exe()
        .or_else(|| std::env::var_os("SC_GUI_SHOTS").map(std::path::PathBuf::from));
    let ask_id = egui::Id::new("uninstall-ask");
    let wipe_id = egui::Id::new("uninstall-wipe");
    let mut asking: bool = ui.data(|d| d.get_temp(ask_id)).unwrap_or(false);
    let mut wipe: bool = ui.data(|d| d.get_temp(wipe_id)).unwrap_or(false);

    section(ui, Some("Uninstall"), |s| {
        let hint = if exe.is_none() {
            "This copy was not installed, so delete its folder to remove it."
        } else {
            "Removes the app and the background service."
        };
        s.row("Uninstall SimpleClip", Some(hint), |ui| {
            if exe.is_some() && widgets::quiet_button(ui, None, "Uninstall\u{2026}").clicked() {
                asking = true;
            }
        });
    });

    let (mut go, mut cancel) = (false, false);
    if asking && exe.is_some() {
        let clips = app.gallery.all_paths().len();
        egui::Window::new("uninstall-window")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .auto_sized()
            .frame(
                egui::Frame::none()
                    .fill(p.card)
                    .rounding(egui::Rounding::same(theme::radius::SHEET))
                    .stroke(egui::Stroke::new(1.0_f32, p.line))
                    .inner_margin(egui::Margin::same(20.0))
                    .shadow(theme::float_shadow(&p)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                ui.label(
                    RichText::new("Are you sure you want to uninstall SimpleClip?")
                        .font(egui::FontId::new(18.0, theme::semibold()))
                        .color(p.text),
                );
                ui.add_space(4.0);
                widgets::secondary(
                    ui,
                    "The app and the background service will be removed. Your settings stay in case you come back.",
                );
                ui.add_space(14.0);
                widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Also delete my clips")
                                    .font(egui::FontId::new(14.0, theme::semibold()))
                                    .color(p.text),
                            );
                            widgets::secondary(
                                ui,
                                if clips == 0 {
                                    "No clips found. They would go to the Recycle Bin.".to_string()
                                } else {
                                    format!(
                                        "Moves {clips} clips and screenshots to the Recycle Bin."
                                    )
                                },
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            widgets::toggle(ui, &mut wipe);
                        });
                    });
                });
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        RichText::new("If you enjoyed SimpleClip, a star on GitHub helps a lot:")
                            .size(13.0)
                            .color(p.text_2),
                    );
                    ui.hyperlink_to(
                        "star the project",
                        "https://github.com/whoevenisxeno/simpleclip-windows",
                    );
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if widgets::primary_button(ui, None, "Uninstall").clicked() {
                        go = true;
                    }
                    if widgets::quiet_button(ui, None, "Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
    }
    if cancel {
        asking = false;
        wipe = false;
    }
    ui.data_mut(|d| {
        d.insert_temp(ask_id, asking);
        d.insert_temp(wipe_id, wipe);
    });
    if go {
        if wipe {
            for path in app.gallery.all_paths() {
                let _ = trash::delete(&path);
            }
        }
        if let Some(exe) = exe {
            crate::update::Updater::start_uninstall(&exe);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn reset_section(app: &mut ScApp, ui: &mut Ui) {
    let id = egui::Id::new("reset-confirm");
    let mut asking: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let mut go = false;
    section(ui, Some("Reset"), |s| {
        let hint = if asking {
            "Sure? Your clips and bookmarks stay. Old settings are saved as config.toml.bak."
        } else {
            "Defaults for everything, then the setup guide. Clips and bookmarks stay."
        };
        s.row("Reset all settings", Some(hint), |ui| {
            if asking {
                if widgets::primary_button(ui, None, "Yes, reset").clicked() {
                    go = true;
                }
                if widgets::quiet_button(ui, None, "Cancel").clicked() {
                    asking = false;
                }
            } else if widgets::quiet_button(ui, None, "Reset\u{2026}").clicked() {
                asking = true;
            }
        });
    });
    if go {
        asking = false;
        if let Ok(path) = sc_core::config::Config::default_path() {
            let _ = std::fs::copy(&path, path.with_extension("toml.bak"));
        }
        app.cfg = sc_core::config::Config::default();
        app.flush_config();
        app.wizard = Default::default();
        app.goto(crate::app::Screen::Wizard);
    }
    ui.data_mut(|d| d.insert_temp(id, asking));
}

fn appearance(app: &mut ScApp, ui: &mut Ui) {
    section(ui, None, |s| {
        s.full("theme appearance look colors colours palette", |ui| {
            let accent = app.cfg.general.accent.clone();
            widgets::theme_picker(ui, &mut app.cfg.general.theme, false, accent.as_deref());
        });
        s.row("Accent color", Some("Laid over any theme"), |ui| {
            widgets::accent_picker(ui, &mut app.cfg.general.accent);
        });
    });
}

fn preset_summary(p: &crate::presets::Preset) -> String {
    format!(
        "{} fps, {} Mbps, {} s replay{}",
        p.fps,
        p.bitrate_kbps / 1000,
        p.replay_secs,
        match p.max_height {
            Some(h) => format!(", {h}p"),
            None => String::new(),
        }
    )
}

fn presets_section(app: &mut ScApp, ui: &mut Ui) {
    use crate::presets;
    let built_in = presets::built_in();
    let user = app.presets.clone();
    let name_id = egui::Id::new("preset-name");
    let mut apply: Option<presets::Preset> = None;
    let mut delete: Option<usize> = None;
    let mut save_as: Option<String> = None;

    section(ui, Some("Presets"), |s| {
        s.row(
            "Preset",
            Some("Quick recording setups. Choosing one changes the settings below."),
            |ui| {
                egui::ComboBox::from_id_salt("preset-choice")
                    .selected_text("Choose\u{2026}")
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for p in built_in.iter().chain(user.iter()) {
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{}  ({})", p.name, preset_summary(p)),
                                )
                                .clicked()
                            {
                                apply = Some(p.clone());
                            }
                        }
                    });
            },
        );
        s.row(
            "Save as a preset",
            Some("Keeps the current frame rate, quality, sound and replay length"),
            |ui| {
                let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
                if widgets::quiet_button(ui, None, "Save").clicked() && !name.trim().is_empty() {
                    save_as = Some(name.trim().to_string());
                    name.clear();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text("Preset name")
                        .desired_width(150.0),
                );
                ui.data_mut(|d| d.insert_temp(name_id, name));
            },
        );
        for (i, p) in user.iter().enumerate() {
            s.row(&p.name, Some(&preset_summary(p)), |ui| {
                if widgets::quiet_button(ui, None, "Delete").clicked() {
                    delete = Some(i);
                }
                if widgets::quiet_button(ui, None, "Apply").clicked() {
                    apply = Some(p.clone());
                }
            });
        }
    });

    if let Some(p) = apply {
        presets::apply(&p, &mut app.cfg);
        app.toast(format!("Applied {}", p.name), crate::app::ToastKind::Ok);
    }
    if let Some(name) = save_as {
        presets::upsert(&mut app.presets, presets::capture(&app.cfg, &name));
        match presets::save(&app.presets) {
            Ok(()) => app.toast(format!("Saved {name}"), crate::app::ToastKind::Ok),
            Err(e) => app.toast(
                format!("Couldn’t save the preset: {e}"),
                crate::app::ToastKind::Error,
            ),
        }
    }
    if let Some(i) = delete {
        if i < app.presets.len() {
            let gone = app.presets.remove(i);
            let _ = presets::save(&app.presets);
            app.toast(format!("Deleted {}", gone.name), crate::app::ToastKind::Ok);
        }
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Monitor,
    Sound,
    Gauge,
    Drive,
    Keys,
    Bell,
    Palette,
    Sliders,
    Star,
}

fn tab_icon(tab: Tab) -> Icon {
    match tab {
        Tab::Capture => Icon::Monitor,
        Tab::Sound => Icon::Sound,
        Tab::Quality => Icon::Gauge,
        Tab::Storage => Icon::Drive,
        Tab::Hotkeys => Icon::Keys,
        Tab::SaveCard => Icon::Bell,
        Tab::Look => Icon::Palette,
        Tab::System => Icon::Sliders,
    }
}

fn arc(center: egui::Pos2, radius: f32, from: f32, to: f32) -> Vec<egui::Pos2> {
    (0..=14)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / 14.0).to_radians();
            egui::pos2(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

fn paint_icon(painter: &egui::Painter, c: egui::Pos2, icon: Icon, color: egui::Color32) {
    let s = egui::Stroke::new(1.5_f32, color);
    let at = |x: f32, y: f32| egui::pos2(c.x + x, c.y + y);
    let line = |pts: &[(f32, f32)], closed: bool| {
        let v: Vec<egui::Pos2> = pts.iter().map(|(x, y)| at(*x, *y)).collect();
        painter.add(if closed {
            egui::Shape::closed_line(v, s)
        } else {
            egui::Shape::line(v, s)
        });
    };
    match icon {
        Icon::Monitor => {
            painter.rect_stroke(
                egui::Rect::from_center_size(at(0.0, -1.0), vec2(14.0, 10.0)),
                2.0,
                s,
            );
            line(&[(0.0, 4.0), (0.0, 6.5)], false);
            line(&[(-3.5, 6.5), (3.5, 6.5)], false);
        }
        Icon::Sound => {
            line(
                &[
                    (-7.0, -2.5),
                    (-4.0, -2.5),
                    (-0.5, -6.0),
                    (-0.5, 6.0),
                    (-4.0, 2.5),
                    (-7.0, 2.5),
                ],
                true,
            );
            painter.add(egui::Shape::line(arc(at(1.0, 0.0), 3.5, -50.0, 50.0), s));
            painter.add(egui::Shape::line(arc(at(1.0, 0.0), 7.0, -50.0, 50.0), s));
        }
        Icon::Gauge => {
            painter.add(egui::Shape::line(arc(at(0.0, 3.5), 7.5, 180.0, 360.0), s));
            line(&[(0.0, 3.5), (3.5, -1.5)], false);
            line(&[(-7.5, 3.5), (-6.0, 3.5)], false);
            line(&[(7.5, 3.5), (6.0, 3.5)], false);
        }
        Icon::Drive => {
            line(&[(-7.0, 1.0), (-5.0, -5.0), (5.0, -5.0), (7.0, 1.0)], false);
            painter.rect_stroke(
                egui::Rect::from_min_max(at(-7.0, 1.0), at(7.0, 6.0)),
                2.0,
                s,
            );
            painter.circle_filled(at(-3.5, 3.5), 1.0, color);
        }
        Icon::Keys => {
            painter.rect_stroke(
                egui::Rect::from_center_size(at(0.0, 0.0), vec2(16.0, 11.0)),
                2.5,
                s,
            );
            for x in [-4.0, 0.0, 4.0] {
                painter.circle_filled(at(x, -1.8), 0.9, color);
            }
            line(&[(-4.0, 2.0), (4.0, 2.0)], false);
        }
        Icon::Bell => {
            line(
                &[
                    (-6.0, 4.5),
                    (-4.5, 3.0),
                    (-4.5, -1.0),
                    (-3.0, -4.0),
                    (0.0, -5.5),
                    (3.0, -4.0),
                    (4.5, -1.0),
                    (4.5, 3.0),
                    (6.0, 4.5),
                ],
                true,
            );
            line(&[(-1.5, 7.0), (1.5, 7.0)], false);
        }
        Icon::Palette => {
            painter.circle_stroke(at(0.0, 0.0), 7.5, s);
            for (x, y) in [(-3.5, -1.5), (0.0, -4.0), (3.5, -1.5)] {
                painter.circle_filled(at(x, y), 1.1, color);
            }
            painter.circle_filled(at(1.5, 3.0), 1.1, color);
        }
        Icon::Sliders => {
            line(&[(-7.0, -3.5), (-4.6, -3.5)], false);
            line(&[(0.6, -3.5), (7.0, -3.5)], false);
            line(&[(-7.0, 3.5), (0.6, 3.5)], false);
            line(&[(5.4, 3.5), (7.0, 3.5)], false);
            painter.circle_stroke(at(-2.0, -3.5), 2.4, s);
            painter.circle_stroke(at(3.0, 3.5), 2.4, s);
        }
        Icon::Star => {
            let mut pts = Vec::new();
            for i in 0..10 {
                let r = if i % 2 == 0 { 7.5 } else { 3.2 };
                let a = (-90.0 + 36.0 * i as f32).to_radians();
                pts.push(at(r * a.cos(), r * a.sin()));
            }
            painter.add(egui::Shape::closed_line(pts, s));
        }
    }
}

fn cleanup(app: &mut ScApp, ui: &mut Ui) {
    let mut scan = false;
    section(ui, Some("Clean up"), |s| {
        s.row(
            "Remove duplicate clips",
            Some("Keeps the compressed copy of a clip, else the trimmed one, else the original."),
            |ui| {
                if widgets::quiet_button(ui, None, "Remove duplicates\u{2026}").clicked() {
                    scan = true;
                }
            },
        );
    });
    if scan {
        let items = gallery::video_files(app)
            .into_iter()
            .map(|(path, bytes)| crate::dupes::Item { path, bytes })
            .collect();
        let found = crate::dupes::plan(items);
        if found.remove.is_empty() {
            app.toast("No duplicate clips found", ToastKind::Ok);
        } else {
            app.settings.dupes = Some(found);
        }
    }
    if app.settings.dupes.is_some() {
        match dupes_dialog(app, &ui.ctx().clone()) {
            Some(true) => remove_duplicates(app),
            Some(false) => app.settings.dupes = None,
            None => {}
        }
    }
}

fn remove_duplicates(app: &mut ScApp) {
    let Some(found) = app.settings.dupes.take() else {
        return;
    };
    let (mut done, mut freed, mut failed) = (0usize, 0u64, 0usize);
    for item in &found.remove {
        if !item.path.exists() {
            continue;
        }
        if gallery::delete(&item.path) {
            app.library.forget(&item.path);
            app.thumbs.forget(&item.path);
            done += 1;
            freed += item.bytes;
        } else {
            failed += 1;
        }
    }
    app.gallery.rescan();
    if failed > 0 {
        app.toast(
            format!("Moved {done} to the Recycle Bin, {failed} could not be moved"),
            ToastKind::Warn,
        );
    } else {
        app.toast(
            format!(
                "Moved {done} {} to the Recycle Bin ({} freed)",
                if done == 1 { "clip" } else { "clips" },
                gallery::size_label(freed)
            ),
            ToastKind::Ok,
        );
    }
}

fn dupes_dialog(app: &mut ScApp, ctx: &egui::Context) -> Option<bool> {
    let plan = app.settings.dupes.clone()?;
    let p = crate::theme::pal(ctx);
    let removing = plan.remove.len();
    let staying: usize = plan.groups.iter().map(|g| g.keep.len()).sum();
    let mut answer = None;
    let closed = crate::screens::sheet::modal(ctx, "dupes-dialog", 600.0, |ui| {
        ui.label(
            RichText::new("Remove duplicate clips?")
                .font(egui::FontId::new(20.0, theme::semibold()))
                .color(p.text),
        );
        ui.add_space(2.0);
        widgets::secondary(
            ui,
            "These are extra versions of clips you also have. For each clip the best version stays: compressed, then trimmed, then the original.",
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let w = ((ui.available_width() - 20.0) / 3.0).floor();
            stat_tile(
                ui,
                w,
                "Will be removed",
                &format!(
                    "{removing} {}",
                    if removing == 1 { "clip" } else { "clips" }
                ),
                p.red,
            );
            stat_tile(
                ui,
                w,
                "Space freed",
                &gallery::size_label(plan.bytes()),
                p.green,
            );
            stat_tile(
                ui,
                w,
                "Staying",
                &format!("{staying} {}", if staying == 1 { "clip" } else { "clips" }),
                p.text,
            );
        });
        ui.add_space(14.0);
        egui::ScrollArea::vertical()
            .id_salt("dupes-list")
            .max_height(300.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for group in &plan.groups {
                    egui::Frame::none()
                        .fill(p.card_hi)
                        .rounding(egui::Rounding::same(theme::radius::CONTROL))
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 8.0;
                            for item in &group.keep {
                                dupe_row(ui, app, item, "Keeps", p.green, false);
                            }
                            for item in &group.remove {
                                dupe_row(ui, app, item, "Removes", p.red, true);
                            }
                        });
                }
            });
        ui.add_space(12.0);
        widgets::secondary(
            ui,
            "They go to the Recycle Bin, so you can still restore them from there.",
        );
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = format!(
                "Move {removing} {} to the Recycle Bin",
                if removing == 1 { "clip" } else { "clips" }
            );
            if widgets::primary_button(ui, None, &label).clicked() {
                answer = Some(true);
            }
            if widgets::quiet_button(ui, None, "Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    if closed && answer.is_none() {
        answer = Some(false);
    }
    answer
}

fn stat_tile(ui: &mut Ui, width: f32, label: &str, value: &str, color: egui::Color32) {
    let p = pal_ui(ui);
    egui::Frame::none()
        .fill(p.card_hi)
        .rounding(egui::Rounding::same(theme::radius::CONTROL))
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new(label.to_uppercase())
                        .font(egui::FontId::new(10.5, theme::semibold()))
                        .color(p.text_3),
                );
                ui.label(
                    RichText::new(value)
                        .font(egui::FontId::new(17.0, theme::semibold()))
                        .color(color),
                );
            });
        });
}

fn dupe_row(
    ui: &mut Ui,
    app: &mut ScApp,
    item: &crate::dupes::Item,
    tag: &str,
    color: egui::Color32,
    muted: bool,
) {
    let p = pal_ui(ui);
    let name = item
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clip")
        .to_string();
    let kind = crate::dupes::kind_of(&item.path);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let size = vec2(64.0, 36.0);
        let tint = if muted {
            egui::Color32::from_white_alpha(120)
        } else {
            egui::Color32::WHITE
        };
        match app.thumbs.get(&item.path, false) {
            Some(tex) => {
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                        .rounding(egui::Rounding::same(6.0))
                        .tint(tint),
                );
            }
            None => {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter()
                    .rect_filled(rect, egui::Rounding::same(6.0), p.card);
            }
        }
        let text_w = (ui.available_width() - 96.0).max(80.0);
        ui.allocate_ui_with_layout(vec2(text_w, 36.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(
                egui::Label::new(RichText::new(&name).size(13.0).color(if muted {
                    p.text_2
                } else {
                    p.text
                }))
                .truncate(),
            );
            ui.label(
                RichText::new(format!(
                    "{} \u{2022} {}",
                    kind.label(),
                    gallery::size_label(item.bytes)
                ))
                .size(12.0)
                .color(p.text_3),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let galley = ui.painter().layout_no_wrap(
                tag.to_string(),
                egui::FontId::new(11.5, theme::semibold()),
                color,
            );
            let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(20.0, 8.0), Sense::hover());
            ui.painter().rect_filled(
                rect,
                egui::Rounding::same(rect.height() / 2.0),
                color.linear_multiply(0.16),
            );
            ui.painter()
                .galley(rect.center() - galley.size() / 2.0, galley, color);
        });
    });
}
