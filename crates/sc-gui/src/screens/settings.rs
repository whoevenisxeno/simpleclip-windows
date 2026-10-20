use crate::app::ScApp;
use crate::theme::{self, pal_ui};
use crate::widgets::{self, section};
use egui::{RichText, Ui};
use sc_core::config::{AudioTracks, FolderPolicy, PopupPosition, PopupStyle};
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

    presets_section(app, ui);
    recording(app, ui);
    audio(app, ui);
    quality(app, ui);
    saving(app, ui);
    shortcuts(app, ui);
    appearance(app, ui);
    general(app, ui);
    ui.add_space(18.0);
    notifications(app, ui);
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
                // Keep the keyframe interval the same length in seconds.
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
                "Native keeps your screen's own size"
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
        s.row(
            "Ask about sharing after trimming",
            Some("Offers to copy the clip or compress it for Discord"),
            |ui| {
                widgets::toggle(ui, &mut app.cfg.general.share_prompt);
            },
        );
        if cfg!(windows) {
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

/// The card that slides in when a clip is saved, and the sound that goes with it.
fn notifications(app: &mut ScApp, ui: &mut Ui) {
    section(ui, Some("Notifications"), |s| {
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
                    // Let the daemon see the new settings first.
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
                    Some("Time between full pictures; shorter makes cuts more exact"),
                    |ui| {
                        // Shown in seconds; stored as frames at the current frame rate.
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

    reset_section(app, ui);
    #[cfg(windows)]
    uninstall_section(app, ui);
}

/// A button at the very bottom to remove SimpleClip. It opens a confirmation
/// that offers to wipe your clips too (off by default) and asks for a GitHub star.
#[cfg(windows)]
fn uninstall_section(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    ui.add_space(18.0);
    let exe = crate::update::Updater::uninstall_exe()
        // The design-review run shows the dialog even from a portable folder.
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
                    .rounding(egui::Rounding::same(14.0))
                    .stroke(egui::Stroke::new(1.0_f32, p.line))
                    .inner_margin(egui::Margin::same(20.0))
                    .shadow(egui::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 28.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(90),
                    }),
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
                // Recycle Bin only: nothing here is deleted for good.
                let _ = trash::delete(&path);
            }
        }
        if let Some(exe) = exe {
            crate::update::Updater::start_uninstall(&exe);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Put every setting back to its default and run the setup guide again. The
/// old settings are kept next to the new file as `config.toml.bak`.
fn reset_section(app: &mut ScApp, ui: &mut Ui) {
    ui.add_space(18.0);
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
        // Keep where clips are saved only if the user chose a folder; everything else resets.
        app.cfg = sc_core::config::Config::default();
        app.flush_config();
        app.wizard = Default::default();
        app.goto(crate::app::Screen::Wizard);
    }
    ui.data_mut(|d| d.insert_temp(id, asking));
}

/// Colour theme and accent. Picking one applies at once.
fn appearance(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    widgets::caption(ui, "Appearance");
    widgets::card(ui, |ui| {
        ui.label(
            RichText::new("Theme")
                .font(egui::FontId::new(14.0, theme::semibold()))
                .color(p.text),
        );
        ui.add_space(4.0);
        widgets::theme_picker(ui, &mut app.cfg.general.theme, false);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Accent color")
                    .font(egui::FontId::new(14.0, theme::semibold()))
                    .color(p.text),
            );
            ui.add_space(8.0);
            widgets::accent_picker(ui, &mut app.cfg.general.accent);
        });
    });
    ui.add_space(18.0);
}

/// One line describing a preset, for its row.
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

/// Built-in and saved recording presets: apply one, or save the current settings.
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
                    .selected_text("Choose...")
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
                format!("Couldn't save the preset: {e}"),
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
    ui.add_space(18.0);
}
