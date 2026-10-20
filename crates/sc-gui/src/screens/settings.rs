use crate::app::ScApp;
use crate::theme::{self, pal_ui};
use crate::widgets::{self, section};
use egui::{RichText, Ui};
use sc_core::config::{AudioTracks, FolderPolicy};
use std::time::Duration;

/// Settings are saved automatically a moment after each change (see `ScApp`).
pub fn view(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    widgets::page_title(ui, "Settings", Some("Changes are saved automatically."));

    if let Some(err) = app.autosave.error.clone() {
        widgets::notice(ui, p.red, &format!("Not saved: {err}"));
        ui.add_space(10.0);
    } else if app
        .autosave
        .saved_at
        .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    {
        widgets::notice(ui, p.green, "Saved");
        ui.add_space(10.0);
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }

    app.devices.ensure(&app.link);

    recording(app, ui);
    audio(app, ui);
    quality(app, ui);
    saving(app, ui);
    shortcuts(app, ui);
    general(app, ui);
    advanced(app, ui);
}

/// Pick one of a few common values; a value that isn't in the list (set by
/// hand in the config file) is shown as an extra option rather than lost.
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
    section(ui, Some("Recording"), |s| {
        if cfg!(windows) {
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
                        (60, "1 min"),
                        (120, "2 min"),
                        (300, "5 min"),
                    ],
                    " s",
                );
            },
        );
        s.row("Frame rate", None, |ui| {
            choice(
                ui,
                &mut app.cfg.capture.target_fps,
                &[(30, "30"), (60, "60"), (120, "120")],
                " fps",
            );
        });
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
    section(ui, Some("Audio"), |s| {
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
        s.row("Microphone", Some("Mix your voice into clips"), |ui| {
            widgets::toggle(ui, &mut app.cfg.audio.mic_enabled);
        });
        if cfg!(windows) && app.cfg.audio.mic_enabled {
            s.row("Input device", None, |ui| {
                let mut id = app.cfg.audio.mic_device.clone().unwrap_or_default();
                device_combo(ui, "mic", &mut id, &app.devices.audio, false);
                app.cfg.audio.mic_device = if id.is_empty() { None } else { Some(id) };
            });
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
    if cfg!(windows) && app.cfg.audio.mic_enabled {
        let p = pal_ui(ui);
        ui.label(
            RichText::new("If the microphone records silence, allow desktop apps in Windows Settings \u{203A} Privacy \u{203A} Microphone.")
                .size(12.0)
                .color(p.text_2),
        );
        ui.add_space(6.0);
    }
}

/// A dropdown of audio endpoints; `outputs` picks speakers vs microphones.
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
    section(ui, Some("Quality"), |s| {
        s.row(
            "Video quality",
            Some(&quality_hint(app.cfg.encode.bitrate_kbps)),
            |ui| {
                choice(
                    ui,
                    &mut app.cfg.encode.bitrate_kbps,
                    &[(8_000, "Standard"), (20_000, "High"), (40_000, "Ultra")],
                    " kbps",
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
    section(ui, Some("Saving"), |s| {
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
    section(ui, Some("Shortcuts"), |s| {
        s.row("Save clip", Some("Works in games and full screen"), |ui| {
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.save, "hk-save");
        });
        s.row("Take screenshot", None, |ui| {
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.screenshot, "hk-shot");
        });
    });
}

fn general(app: &mut ScApp, ui: &mut Ui) {
    section(ui, Some("General"), |s| {
        if cfg!(windows) {
            s.row(
                "Open at login",
                Some("Runs quietly in the background so the buffer is always ready"),
                |ui| {
                    widgets::toggle(ui, &mut app.cfg.general.autostart);
                },
            );
        }
        s.row("Show a card when a clip is saved", None, |ui| {
            widgets::toggle(ui, &mut app.cfg.general.notify);
        });
        s.row("Play a sound when a clip is saved", None, |ui| {
            widgets::toggle(ui, &mut app.cfg.general.save_sound);
        });
    });
}

fn advanced(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    egui::CollapsingHeader::new(RichText::new("Advanced").size(13.0).color(p.text_2))
        .id_salt("advanced")
        .show(ui, |ui| {
            ui.add_space(6.0);
            section(ui, None, |s| {
                s.row("File name", Some("Use {app}, {date} and {time}"), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut app.cfg.save.filename_template)
                            .desired_width(220.0),
                    );
                });
                s.row(
                    "Keyframe interval",
                    Some("Frames between full pictures; shorter makes cuts more exact"),
                    |ui| {
                        choice(
                            ui,
                            &mut app.cfg.encode.gop_frames,
                            &[(60, "1 s"), (120, "2 s"), (240, "4 s")],
                            " frames",
                        );
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
                        Some("Look on GitHub for a newer version"),
                        |ui| {
                            widgets::toggle(ui, &mut app.cfg.general.update_check);
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
        });

    #[cfg(windows)]
    uninstall_section(app, ui);
}

/// A button at the very bottom to remove SimpleClip (asks first).
#[cfg(windows)]
fn uninstall_section(app: &mut ScApp, ui: &mut Ui) {
    ui.add_space(18.0);
    let _ = app;
    let exe = crate::update::Updater::uninstall_exe();
    let id = egui::Id::new("uninstall-confirm");
    let mut asking: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let mut go = false;
    section(ui, Some("Uninstall"), |s| {
        let hint = match (&exe, asking) {
            (None, _) => "This copy was not installed, so delete its folder to remove it.",
            (_, true) => {
                "Your clips, bookmarks and settings are kept. Remove them by hand if you want."
            }
            _ => "Removes the app and the background service. Your clips are kept.",
        };
        s.row("Uninstall SimpleClip", Some(hint), |ui| {
            if exe.is_none() {
                return;
            }
            if asking {
                if widgets::primary_button(ui, None, "Yes, uninstall").clicked() {
                    go = true;
                }
                if widgets::quiet_button(ui, None, "Cancel").clicked() {
                    asking = false;
                }
            } else if widgets::quiet_button(ui, None, "Uninstall{2026}").clicked() {
                asking = true;
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(id, asking));
    if go {
        if let Some(exe) = exe {
            crate::update::Updater::start_uninstall(&exe);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
