use crate::app::{ScApp, ToastKind};
use crate::screens::gallery;
use crate::theme::{self, pal_ui};
use crate::widgets::{self, section};
use egui::{vec2, Align, Layout, RichText, Sense, Ui};
use sc_core::config::{
    describe_secs, AudioTracks, EventsSettings, ExtraHotkey, FolderPolicy, HotkeyAction,
    HotkeySettings, PopupPosition, PopupStyle, MAX_EXTRA_HOTKEYS, MAX_VOLUME_PERCENT,
    SAVE_LAST_CHOICES,
};
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
    Addons,
    System,
}

const TABS: [(Tab, &str, &str); 9] = [
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
        Tab::Addons,
        "Add-ons",
        "Optional extras. Nothing here is installed until you turn it on.",
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
    #[cfg(windows)]
    pub addon: crate::addon::Events,
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
        rail(ui, height, |ui| {
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

fn rail(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(168.0, height), Layout::top_down(Align::Min), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("settings-rail")
            .max_height(height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                add(ui);
            });
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
        Tab::Addons => {
            #[cfg(windows)]
            addons_section(app, ui);
            #[cfg(not(windows))]
            let _ = (app, ui);
        }
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
    if cfg!(windows) {
        extra_shortcuts(
            &mut app.cfg.hotkeys,
            app.cfg.buffer.replay_duration_secs,
            ui,
        );
    }
}

fn extra_shortcuts(h: &mut HotkeySettings, buffer_secs: u32, ui: &mut Ui) {
    let p = pal_ui(ui);
    let mut remove = None;
    let mut add = false;
    let count = h.extra.len();
    section(ui, Some("More shortcuts"), |s| {
        s.row(
            "Your own shortcuts",
            Some("Another clip length, a screenshot, pause and more"),
            |ui| {
                let room = count < MAX_EXTRA_HOTKEYS;
                if ui
                    .add_enabled_ui(room, |ui| {
                        widgets::quiet_button(ui, Some(theme::icon::ADD), "Add a shortcut")
                    })
                    .inner
                    .clicked()
                {
                    add = true;
                }
            },
        );
        for i in 0..count {
            let duplicate = h.duplicate_of(i);
            let entry = &mut h.extra[i];
            let hint = if let Some(other) = duplicate {
                Some(format!("Already used by “{other}”, so this one is off"))
            } else if entry.keys.trim().is_empty() {
                Some("Press Change to pick the keys".to_string())
            } else if entry.action == HotkeyAction::SaveLast && entry.secs > buffer_secs {
                Some(format!(
                    "Your replay length is {}, so this saves at most that much",
                    describe_secs(buffer_secs)
                ))
            } else {
                None
            };
            let keywords = entry.describe();
            let hint = hint.as_deref();
            s.pair(
                &keywords,
                hint,
                |ui| {
                    egui::ComboBox::from_id_salt(("hk-action", i))
                        .selected_text(entry.action.label())
                        .width(210.0)
                        .height(360.0)
                        .show_ui(ui, |ui| {
                            for action in HotkeyAction::ALL {
                                ui.selectable_value(&mut entry.action, action, action.label());
                            }
                        });
                    if entry.action == HotkeyAction::SaveLast {
                        let mut options = SAVE_LAST_CHOICES.to_vec();
                        if !options.contains(&entry.secs) {
                            options.push(entry.secs);
                            options.sort_unstable();
                        }
                        egui::ComboBox::from_id_salt(("hk-secs", i))
                            .selected_text(describe_secs(entry.secs))
                            .width(110.0)
                            .height(360.0)
                            .show_ui(ui, |ui| {
                                for secs in options {
                                    ui.selectable_value(&mut entry.secs, secs, describe_secs(secs));
                                }
                            });
                    }
                },
                |ui| {
                    if widgets::icon_button(ui, theme::icon::DELETE, "Remove", "Remove", true)
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    widgets::hotkey_field(ui, &mut entry.keys, &format!("hk-extra-{i}"));
                    if entry.keys.trim().is_empty() {
                        ui.label(RichText::new("Not set").size(12.5).color(p.text_3));
                    }
                },
            );
        }
    });
    if add {
        h.extra.push(ExtraHotkey::default());
    }
    if let Some(i) = remove {
        h.extra.remove(i);
    }
}

#[cfg(windows)]
fn addons_section(app: &mut ScApp, ui: &mut Ui) {
    use crate::addon::Finished;
    app.settings.addon.refresh();
    if let Some(done) = app.settings.addon.poll() {
        match done {
            Finished::Installed(_) => {
                app.cfg.events.enabled = true;
                app.toast("Game events are on", ToastKind::Ok);
            }
            Finished::Removed => {
                app.cfg.events.enabled = false;
                app.toast("The game events add-on was removed", ToastKind::Ok);
            }
            Finished::Failed(e) => app.toast(e, ToastKind::Warn),
        }
    }
    addons_card(&mut app.cfg.events, &mut app.settings.addon, ui);
}

#[cfg(windows)]
fn addons_card(events: &mut EventsSettings, addon: &mut crate::addon::Events, ui: &mut Ui) {
    use crate::addon::{cs2_text, dota_text, league_text, r6_text, Busy};
    let p = pal_ui(ui);
    let busy = addon.busy();
    let installed = addon.installed().is_some();
    let status = addon.status().cloned();
    let (mut ask, mut remove) = (false, false);
    let cs2_line = cs2_text(status.as_ref(), events.enabled && events.cs2);
    let league_line = league_text(status.as_ref(), events.enabled && events.league);
    let r6_line = r6_text(status.as_ref(), events.enabled && events.r6);
    let dota_line = dota_text(status.as_ref(), events.enabled && events.dota2);
    section(ui, None, |s| {
        s.row(
            "Game events",
            Some("Experimental. Saves a clip on its own after a multi-kill"),
            |ui| match (&busy, installed) {
                (Some(Busy::Working { label, percent }), _) => {
                    let text = match percent {
                        Some(n) => format!("{label}\u{2026} {n}%"),
                        None => format!("{label}\u{2026}"),
                    };
                    ui.label(RichText::new(text).size(13.0).color(p.text_2));
                }
                (None, false) => {
                    if widgets::quiet_button(ui, None, "Turn on\u{2026}").clicked() {
                        ask = true;
                    }
                }
                (None, true) => {
                    widgets::toggle(ui, &mut events.enabled);
                    if widgets::quiet_button(ui, None, "Remove").clicked() {
                        remove = true;
                    }
                }
            },
        );
    });
    if installed && busy.is_none() {
        ui.add_enabled_ui(events.enabled, |ui| {
            section(ui, Some("Games"), |s| {
                s.row("Rainbow Six Siege", Some(&r6_line), |ui| {
                    widgets::toggle(ui, &mut events.r6);
                });
                s.row("Counter-Strike 2", Some(&cs2_line), |ui| {
                    widgets::toggle(ui, &mut events.cs2);
                });
                s.row("Dota 2", Some(&dota_line), |ui| {
                    widgets::toggle(ui, &mut events.dota2);
                });
                s.row("League of Legends", Some(&league_line), |ui| {
                    widgets::toggle(ui, &mut events.league);
                });
            });
            section(ui, Some("Saving"), |s| {
                s.row(
                    "Save a clip at",
                    Some("The smallest streak that counts"),
                    |ui| {
                        widgets::segmented(
                            ui,
                            &mut events.min_kills,
                            &[
                                (1, "Any kill"),
                                (2, "Double"),
                                (3, "Triple"),
                                (4, "Quadra"),
                                (5, "Penta"),
                            ],
                        );
                    },
                );
                s.row(
                    "Clip length",
                    Some("How much before the moment is saved"),
                    |ui| {
                        widgets::segmented(
                            ui,
                            &mut events.save_secs,
                            &[
                                (10, "10 s"),
                                (15, "15 s"),
                                (20, "20 s"),
                                (30, "30 s"),
                                (60, "1 min"),
                            ],
                        );
                    },
                );
                s.row(
                    "Test mode",
                    Some("Shows a notification instead of saving a clip"),
                    |ui| {
                        widgets::toggle(ui, &mut events.test_mode);
                    },
                );
            });
        });
    }
    if ask {
        addon.asking = true;
    }
    if remove {
        events.enabled = false;
        addon.start_remove(ui.ctx());
    }
    if addon.asking {
        let (mut go, mut cancel) = (false, false);
        egui::Window::new("addon-window")
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
                ui.set_width(460.0);
                ui.label(
                    RichText::new("Turn on game events?")
                        .font(egui::FontId::new(18.0, theme::semibold()))
                        .color(p.text),
                );
                ui.add_space(6.0);
                widgets::secondary(
                    ui,
                    "This is an optional add-on, so it is not part of SimpleClip until you choose it. SimpleClip will download it from the SimpleClip releases on GitHub, check its checksum, and install it in your SimpleClip data folder.",
                );
                ui.add_space(8.0);
                widgets::secondary(
                    ui,
                    "It saves a clip by itself when you get a multi-kill in Rainbow Six Siege, Counter-Strike 2, Dota 2 or League of Legends. For CS2 and Dota 2 it adds one small file to the game's cfg folder. For Rainbow Six Siege it reads the kill feed on your screen, and only looks for your own kills. It only talks to your own computer. You can turn it off or remove it here at any time.",
                );
                ui.add_space(8.0);
                widgets::secondary(
                    ui,
                    "It is experimental, so it may miss a kill or save one you did not expect.",
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if widgets::primary_button(ui, None, "Download and turn on").clicked() {
                        go = true;
                    }
                    if widgets::quiet_button(ui, None, "Not now").clicked() {
                        cancel = true;
                    }
                });
            });
        if go {
            addon.start_install(ui.ctx());
        }
        if cancel {
            addon.asking = false;
        }
    }
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
                Some("Looks on GitHub every 6 minutes for a newer version"),
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
                    app.updater.check_now(ui.ctx());
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
    Addons,
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
        Tab::Addons => Icon::Addons,
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
        Icon::Addons => {
            painter.rect_stroke(
                egui::Rect::from_center_size(at(0.0, 0.0), vec2(13.0, 13.0)),
                3.0,
                s,
            );
            line(&[(-3.5, 0.0), (3.5, 0.0)], false);
            line(&[(0.0, -3.5), (0.0, 3.5)], false);
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
    let mut answer = None;
    let closed = crate::screens::sheet::modal(ctx, "dupes-dialog", 600.0, |ui| {
        answer = dupes_body(ui, &plan, &mut |ui, item, tag, color, muted| {
            dupe_row(ui, app, item, tag, color, muted)
        });
    });
    if closed && answer.is_none() {
        answer = Some(false);
    }
    answer
}

type DupeRow<'a> = dyn FnMut(&mut Ui, &crate::dupes::Item, &str, egui::Color32, bool) + 'a;

fn dupes_body(ui: &mut Ui, plan: &crate::dupes::Plan, row: &mut DupeRow) -> Option<bool> {
    let p = pal_ui(ui);
    let removing = plan.remove.len();
    let staying: usize = plan.groups.iter().map(|g| g.keep.len()).sum();
    let list = (ui.ctx().screen_rect().height() - 340.0).clamp(90.0, 300.0);
    let mut answer = None;
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
        .max_height(list)
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
                            row(ui, item, "Keeps", p.green, false);
                        }
                        for item in &group.remove {
                            row(ui, item, "Removes", p.red, true);
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

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod shortcut_tests {
    use super::*;
    use crate::testkit::Harness;

    fn entry(keys: &str, action: HotkeyAction, secs: u32) -> ExtraHotkey {
        ExtraHotkey {
            keys: keys.into(),
            action,
            secs,
        }
    }

    fn open(h: &mut Harness, hk: &mut HotkeySettings, buffer: u32) {
        for _ in 0..3 {
            h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                extra_shortcuts(hk, buffer, ui)
            });
        }
    }

    fn click(h: &mut Harness, hk: &mut HotkeySettings, buffer: u32, text: &str, nth: usize) {
        h.click_nth(text, nth, &mut |ui| extra_shortcuts(hk, buffer, ui));
        open(h, hk, buffer);
    }

    #[test]
    fn the_section_starts_with_just_an_add_button() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        open(&mut h, &mut hk, 30);
        assert!(h.has("Your own shortcuts"));
        assert!(h.has("Add a shortcut"));
        assert!(!h.has("Not set"));
    }

    #[test]
    fn adding_a_shortcut_starts_as_save_the_last_thirty_seconds() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        open(&mut h, &mut hk, 30);
        click(&mut h, &mut hk, 30, "Add a shortcut", 0);
        assert_eq!(hk.extra, vec![ExtraHotkey::default()]);
        assert!(h.has("Save the last…"));
        assert!(h.has("30 seconds"));
        assert!(h.has("Not set"));
        assert!(h.has("Press Change to pick the keys"));
    }

    #[test]
    fn removing_one_leaves_the_others() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        hk.extra = vec![
            entry("CTRL+F1", HotkeyAction::SaveLast, 15),
            entry("CTRL+F2", HotkeyAction::Screenshot, 30),
        ];
        open(&mut h, &mut hk, 30);
        click(&mut h, &mut hk, 30, theme::icon::DELETE, 0);
        assert_eq!(hk.extra.len(), 1);
        assert_eq!(hk.extra[0].keys, "CTRL+F2");
        assert_eq!(hk.extra[0].action, HotkeyAction::Screenshot);
    }

    #[test]
    fn the_add_button_stops_at_the_limit() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        hk.extra = (0..MAX_EXTRA_HOTKEYS)
            .map(|i| entry(&format!("CTRL+F{}", i + 1), HotkeyAction::Screenshot, 30))
            .collect();
        open(&mut h, &mut hk, 30);
        click(&mut h, &mut hk, 30, "Add a shortcut", 0);
        assert_eq!(hk.extra.len(), MAX_EXTRA_HOTKEYS);
    }

    #[test]
    fn a_repeated_key_says_what_it_clashes_with() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        hk.extra = vec![entry(&hk.save.clone(), HotkeyAction::Screenshot, 30)];
        open(&mut h, &mut hk, 30);
        assert!(h.has("Already used by “Save clip”, so this one is off"));
    }

    #[test]
    fn a_length_longer_than_the_replay_buffer_is_flagged() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        hk.extra = vec![entry("CTRL+F1", HotkeyAction::SaveLast, 120)];
        open(&mut h, &mut hk, 30);
        assert!(h.has("Your replay length is 30 seconds, so this saves at most that much"));
        let mut hk2 = HotkeySettings::default();
        hk2.extra = vec![entry("CTRL+F1", HotkeyAction::SaveLast, 120)];
        let mut h2 = Harness::new();
        open(&mut h2, &mut hk2, 300);
        assert!(!h2.has("Your replay length is 5 minutes, so this saves at most that much"));
    }

    #[test]
    fn the_action_can_be_changed_and_the_length_follows() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        hk.extra = vec![entry("CTRL+F1", HotkeyAction::SaveLast, 60)];
        open(&mut h, &mut hk, 300);
        assert!(h.has("1 minute"));
        click(&mut h, &mut hk, 300, "Save the last…", 0);
        for action in HotkeyAction::ALL {
            assert!(h.has(action.label()), "{}", action.label());
        }
        click(&mut h, &mut hk, 300, "Pause or resume recording", 0);
        assert_eq!(hk.extra[0].action, HotkeyAction::TogglePause);
        assert!(!h.has("1 minute"));
    }

    #[test]
    fn the_length_can_be_picked_from_the_choices() {
        let mut h = Harness::new();
        let mut hk = HotkeySettings::default();
        hk.extra = vec![entry("CTRL+F1", HotkeyAction::SaveLast, 30)];
        open(&mut h, &mut hk, 300);
        click(&mut h, &mut hk, 300, "30 seconds", 0);
        for secs in SAVE_LAST_CHOICES {
            assert!(h.has(&describe_secs(secs)), "{secs}");
        }
        click(&mut h, &mut hk, 300, "2 minutes", 0);
        assert_eq!(hk.extra[0].secs, 120);
    }
}

#[cfg(all(test, windows))]
mod addon_card_tests {
    use super::*;
    use crate::addon::Events;
    use crate::testkit::Harness;
    use sc_core::addon::{now_secs, EventsStatus};

    fn frames(h: &mut Harness, ev: &mut EventsSettings, addon: &mut Events) {
        for _ in 0..3 {
            h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                addons_card(ev, addon, ui)
            });
        }
    }

    fn click(h: &mut Harness, ev: &mut EventsSettings, addon: &mut Events, text: &str) {
        h.click_text(text, &mut |ui| addons_card(ev, addon, ui));
        frames(h, ev, addon);
    }

    fn live(cs2: &str, league: &str) -> EventsStatus {
        EventsStatus {
            version: "9.9.9".into(),
            cs2: cs2.into(),
            league: league.into(),
            last_event: None,
            updated: now_secs(),
            ..Default::default()
        }
    }

    #[test]
    fn before_it_is_installed_there_is_only_a_turn_on_button() {
        let mut h = Harness::new();
        let (mut ev, mut addon) = (EventsSettings::default(), Events::default());
        frames(&mut h, &mut ev, &mut addon);
        assert!(h.has("Game events"));
        assert!(h.has("Turn on…"));
        assert!(!h.has("Counter-Strike 2"));
        assert!(!h.has("Remove"));
        assert!(!h.has("Turn on game events?"));
    }

    #[test]
    fn turning_on_asks_first_and_not_now_changes_nothing() {
        let mut h = Harness::new();
        let (mut ev, mut addon) = (EventsSettings::default(), Events::default());
        frames(&mut h, &mut ev, &mut addon);
        click(&mut h, &mut ev, &mut addon, "Turn on…");
        assert!(addon.asking);
        assert!(h.has("Turn on game events?"));
        assert!(h.has("Download and turn on"));
        assert!(h.has("Not now"));
        click(&mut h, &mut ev, &mut addon, "Not now");
        assert!(!addon.asking);
        assert!(!h.has("Turn on game events?"));
        assert!(!ev.enabled);
        assert!(addon.busy().is_none());
    }

    #[test]
    fn once_installed_the_options_appear() {
        let mut h = Harness::new();
        let (mut ev, mut addon) = (EventsSettings::default(), Events::default());
        addon.pretend(Some("9.9.9"), Some(live("connected", "in-game")));
        ev.enabled = true;
        frames(&mut h, &mut ev, &mut addon);
        assert!(h.has("Remove"));
        assert!(h.has("Counter-Strike 2"));
        assert!(h.has("Connected to the game"));
        assert!(h.has("League of Legends"));
        assert!(h.has("In a match"));
        assert!(h.has("Save a clip at"));
        assert!(h.has("Clip length"));
        assert!(!h.has("Turn on…"));
    }

    #[test]
    fn the_status_lines_are_off_while_the_add_on_is_off() {
        let mut h = Harness::new();
        let (mut ev, mut addon) = (EventsSettings::default(), Events::default());
        addon.pretend(Some("9.9.9"), Some(live("connected", "in-game")));
        frames(&mut h, &mut ev, &mut addon);
        assert!(!h.has("Connected to the game"));
        assert!(h.has("Off"));
    }

    #[test]
    fn the_streak_and_the_clip_length_can_be_chosen() {
        let mut h = Harness::new();
        let (mut ev, mut addon) = (EventsSettings::default(), Events::default());
        addon.pretend(Some("9.9.9"), None);
        ev.enabled = true;
        frames(&mut h, &mut ev, &mut addon);
        assert_eq!(ev.min_kills, 2);
        click(&mut h, &mut ev, &mut addon, "Triple");
        assert_eq!(ev.min_kills, 3);
        click(&mut h, &mut ev, &mut addon, "Any kill");
        assert_eq!(ev.min_kills, 1);
        click(&mut h, &mut ev, &mut addon, "30 s");
        assert_eq!(ev.save_secs, 30);
        click(&mut h, &mut ev, &mut addon, "1 min");
        assert_eq!(ev.save_secs, 60);
        assert!(ev.validate().is_ok());
    }

    #[test]
    fn a_game_with_a_setup_problem_says_so() {
        let mut h = Harness::new();
        let (mut ev, mut addon) = (EventsSettings::default(), Events::default());
        ev.enabled = true;
        addon.pretend(Some("9.9.9"), Some(live("not-found", "waiting")));
        frames(&mut h, &mut ev, &mut addon);
        assert!(h.has("Not found in your Steam folders"));
        assert!(h.has("Waiting for a match"));
    }
}

#[cfg(all(test, windows))]
mod addon_games_tests {
    use super::*;
    use crate::addon::Events;
    use crate::testkit::Harness;
    use sc_core::addon::{now_secs, EventsStatus};

    fn frames(h: &mut Harness, ev: &mut EventsSettings, addon: &mut Events) {
        for _ in 0..3 {
            h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                addons_card(ev, addon, ui)
            });
        }
    }

    #[test]
    fn every_game_has_a_row_with_its_own_status() {
        let mut h = Harness::new();
        let mut ev = EventsSettings {
            enabled: true,
            ..Default::default()
        };
        let mut addon = Events::default();
        addon.pretend(
            Some("9.9.9"),
            Some(EventsStatus {
                r6: "watching".into(),
                dota2: "waiting".into(),
                cs2: "connected".into(),
                league: "in-game".into(),
                updated: now_secs(),
                ..Default::default()
            }),
        );
        frames(&mut h, &mut ev, &mut addon);
        for name in [
            "Rainbow Six Siege",
            "Counter-Strike 2",
            "Dota 2",
            "League of Legends",
        ] {
            assert!(h.has(name), "{name}");
        }
        assert!(h.has("Watching the kill feed"));
        assert!(h.has("Connected to the game"));
        assert!(h.has("In a match"));
    }

    #[test]
    fn each_game_can_be_switched_off_on_its_own() {
        let mut h = Harness::new();
        let mut ev = EventsSettings {
            enabled: true,
            r6: false,
            dota2: false,
            ..Default::default()
        };
        let mut addon = Events::default();
        addon.pretend(Some("9.9.9"), None);
        frames(&mut h, &mut ev, &mut addon);
        assert!(h.has("Rainbow Six Siege"));
        assert!(h.has("Off"));
    }
}

#[cfg(all(test, windows))]
mod addon_test_mode_tests {
    use super::*;
    use crate::addon::Events;
    use crate::testkit::Harness;

    fn frames(h: &mut Harness, ev: &mut EventsSettings, addon: &mut Events) {
        for _ in 0..3 {
            h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                addons_card(ev, addon, ui)
            });
        }
    }

    #[test]
    fn test_mode_has_a_switch_that_starts_off() {
        let mut h = Harness::new();
        let mut ev = EventsSettings {
            enabled: true,
            ..Default::default()
        };
        let mut addon = Events::default();
        addon.pretend(Some("9.9.9"), None);
        frames(&mut h, &mut ev, &mut addon);
        assert!(h.has("Test mode"));
        assert!(h.has("Shows a notification instead of saving a clip"));
        assert!(!ev.test_mode);
    }

    #[test]
    fn clicking_the_switch_turns_test_mode_on_and_off() {
        let mut h = Harness::new();
        let mut ev = EventsSettings {
            enabled: true,
            ..Default::default()
        };
        let mut addon = Events::default();
        addon.pretend(Some("9.9.9"), None);
        frames(&mut h, &mut ev, &mut addon);
        let row = h.at("Test mode", 0).unwrap();
        let toggle_at = egui::pos2(h.right - 40.0, row.center().y);
        h.click(toggle_at, &mut |ui| addons_card(&mut ev, &mut addon, ui));
        assert!(ev.test_mode);
        frames(&mut h, &mut ev, &mut addon);
        let row = h.at("Test mode", 0).unwrap();
        let toggle_at = egui::pos2(h.right - 40.0, row.center().y);
        h.click(toggle_at, &mut |ui| addons_card(&mut ev, &mut addon, ui));
        assert!(!ev.test_mode);
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;
    use crate::testkit::Harness;

    fn plan() -> crate::dupes::Plan {
        let item = |name: &str, mb: u64| crate::dupes::Item {
            path: std::path::PathBuf::from(format!("{name}.mp4")),
            bytes: mb * 1_048_576,
        };
        crate::dupes::plan(vec![
            item("sc-apex-20261026-193000", 52),
            item("sc-apex-20261026-193000-trim", 14),
            item("sc-apex-20261026-193000-trim-10mb", 9),
            item("sc-valorant-20261027-211500", 61),
            item("sc-valorant-20261027-211500-25mb", 24),
            item("sc-cs2-20261026-180000", 48),
            item("sc-cs2-20261026-180000-trim", 12),
        ])
    }

    #[test]
    fn the_duplicates_dialog_keeps_its_title_and_buttons_inside_the_window() {
        let found = plan();
        for height in [470.0, 640.0, 820.0] {
            let mut h = Harness::new().sized(1080.0, height);
            for _ in 0..4 {
                h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                    crate::screens::sheet::modal(ui.ctx(), "dupes-dialog", 600.0, |ui| {
                        dupes_body(ui, &found, &mut |ui, item, _, _, _| {
                            let w = ui.available_width();
                            ui.allocate_ui(vec2(w, 36.0), |ui| {
                                ui.label(item.path.display().to_string());
                            });
                        });
                    });
                });
            }
            let title = h
                .at("Remove duplicate clips?", 0)
                .expect("the title is drawn");
            let cancel = h.at("Cancel", 0).expect("the buttons are drawn");
            assert!(
                title.top() >= 0.0,
                "the title starts at {} at {height}",
                title.top()
            );
            assert!(
                cancel.bottom() <= height,
                "the buttons end at {} in a window {height} tall",
                cancel.bottom()
            );
        }
    }

    #[test]
    fn a_short_window_keeps_the_settings_rail_inside_its_height() {
        let mut h = Harness::new().sized(1000.0, 470.0);
        for _ in 0..3 {
            h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
                rail(ui, 300.0, |ui| {
                    for (tab, label, _) in TABS {
                        rail_item(ui, label, false, tab_icon(tab));
                    }
                    rail_item(ui, "Setup guide", false, Icon::Star);
                });
            });
        }
        let bottom = h.at("Capture", 0).expect("the first tab is drawn").top() + 300.0;
        for (_, label, _) in TABS {
            if let Some(r) = h.at(label, 0) {
                assert!(
                    r.top() < bottom,
                    "{label} starts at {} below {bottom}",
                    r.top()
                );
            }
        }
        assert!(!h.has("Setup guide"));
    }
}
