use crate::app::ScApp;
use crate::{config_io, theme, widgets};
use sc_core::config::{AudioTracks, Container, FolderPolicy};
use sc_core::encode::Codec;

/// Editing the same TOML the daemon hot-reloads. Saving here triggers the
/// daemon's reload path; no restart needed.
pub fn view(app: &mut ScApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.heading("Settings");
    ui.colored_label(theme::MUTED, config_io::path().display().to_string());
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        capture(app, ui);
        ui.add_space(8.0);
        buffer(app, ui);
        ui.add_space(8.0);
        quality(app, ui);
        ui.add_space(8.0);
        audio(app, ui);
        ui.add_space(8.0);
        save(app, ui);
        ui.add_space(8.0);
        hotkey(app, ui);
        ui.add_space(8.0);
        feedback(app, ui);
        ui.add_space(12.0);
        save_row(app, ui);
        ui.add_space(12.0);
    });
}

fn capture(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Capture", |ui| {
        if cfg!(windows) {
            ensure_devices(app);
            let current = app.cfg.capture.monitor_id.clone();
            let label = app
                .devices
                .monitors
                .iter()
                .find(|m| m.id == current)
                .map(monitor_label)
                .unwrap_or_else(|| "Primary display".to_string());
            ui.horizontal(|ui| {
                ui.label("Display:");
                egui::ComboBox::from_id_salt("monitor")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.cfg.capture.monitor_id,
                            String::new(),
                            "Primary display",
                        );
                        for m in &app.devices.monitors {
                            ui.selectable_value(
                                &mut app.cfg.capture.monitor_id,
                                m.id.clone(),
                                monitor_label(m),
                            );
                        }
                    });
                if ui.small_button("Refresh").clicked() {
                    app.devices.refresh();
                }
            });
        }
        ui.add(egui::Slider::new(&mut app.cfg.capture.target_fps, 15..=240).text("target FPS"));
        ui.checkbox(&mut app.cfg.capture.show_cursor, "Capture the mouse cursor");
    });
}

fn monitor_label(m: &sc_core::capture::MonitorInfo) -> String {
    let primary = if m.primary { ", primary" } else { "" };
    format!("{} ({}x{}{primary})", m.name, m.width, m.height)
}

/// Fetch displays/audio outputs from the daemon the first time they're needed.
fn ensure_devices(app: &mut ScApp) {
    if !app.devices.loaded && matches!(app.link, crate::daemon::Link::Connected(_)) {
        app.devices.refresh();
    }
}

fn buffer(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Replay buffer", |ui| {
        ui.add(
            egui::Slider::new(&mut app.cfg.buffer.replay_duration_secs, 5..=300)
                .text("replay seconds"),
        );
        ui.add(egui::Slider::new(&mut app.cfg.buffer.ram_cap_mb, 256..=8192).text("RAM cap (MB)"));
        let est = app.cfg.estimated_buffer_mb();
        let over = est > app.cfg.buffer.ram_cap_mb;
        ui.colored_label(
            if over { theme::DANGER } else { theme::MUTED },
            format!("estimated ~{est} MB"),
        );
    });
}

fn quality(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Quality", |ui| {
        ui.add(
            egui::Slider::new(&mut app.cfg.encode.bitrate_kbps, 2_000..=80_000)
                .text("bitrate kbps"),
        );
        let mbmin = app.cfg.encode.bitrate_kbps as f32 / 8.0 * 60.0 / 1000.0;
        ui.colored_label(theme::MUTED, format!("~{mbmin:.0} MB per minute"));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Codec:");
            ui.selectable_value(&mut app.cfg.encode.codec, Codec::H264, "H.264");
            ui.selectable_value(&mut app.cfg.encode.codec, Codec::Hevc, "HEVC");
            ui.selectable_value(&mut app.cfg.encode.codec, Codec::Av1, "AV1");
        });
        if app.cfg.encode.codec != Codec::H264 {
            ui.colored_label(theme::WARN, "HEVC/AV1 may not upload or play everywhere.");
        }
        ui.add(
            egui::Slider::new(&mut app.cfg.encode.gop_frames, 15..=300)
                .text("keyframe interval (frames)"),
        );
    });
}

fn audio(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Audio", |ui| {
        ui.checkbox(&mut app.cfg.audio.desktop_enabled, "Capture desktop audio");
        if cfg!(windows) && app.cfg.audio.desktop_enabled {
            ensure_devices(app);
            let current = app.cfg.audio.desktop_device.clone();
            let label = app
                .devices
                .audio
                .iter()
                .find(|d| d.id == current)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "Default output device".to_string());
            ui.horizontal(|ui| {
                ui.label("Output device:");
                egui::ComboBox::from_id_salt("audio-out")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.cfg.audio.desktop_device,
                            String::new(),
                            "Default output device",
                        );
                        for d in &app.devices.audio {
                            ui.selectable_value(
                                &mut app.cfg.audio.desktop_device,
                                d.id.clone(),
                                d.name.clone(),
                            );
                        }
                    });
            });
        }
        ui.checkbox(&mut app.cfg.audio.mic_enabled, "Capture microphone");
        if app.cfg.audio.mic_enabled {
            ui.horizontal(|ui| {
                ui.label("Tracks:");
                ui.selectable_value(&mut app.cfg.audio.tracks, AudioTracks::Mixed, "Mixed");
                ui.selectable_value(&mut app.cfg.audio.tracks, AudioTracks::Separate, "Separate");
            });
        }
    });
}

fn save(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Saving", |ui| {
        let mut dir = app
            .cfg
            .save
            .directory
            .clone()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        ui.label("Folder (blank = Videos):");
        if ui.text_edit_singleline(&mut dir).changed() {
            app.cfg.save.directory = if dir.trim().is_empty() {
                None
            } else {
                Some(dir.trim().into())
            };
        }
        ui.label("Filename template ({app} {date} {time}):");
        ui.text_edit_singleline(&mut app.cfg.save.filename_template);
        ui.horizontal(|ui| {
            ui.label("Subfolders:");
            ui.selectable_value(
                &mut app.cfg.save.folder_policy,
                FolderPolicy::PerDay,
                "Per day",
            );
            ui.selectable_value(
                &mut app.cfg.save.folder_policy,
                FolderPolicy::PerApp,
                "Per app",
            );
            ui.selectable_value(&mut app.cfg.save.folder_policy, FolderPolicy::Flat, "None");
        });
        ui.horizontal(|ui| {
            ui.label("Container:");
            ui.selectable_value(&mut app.cfg.save.container, Container::Mp4, "MP4");
            ui.selectable_value(&mut app.cfg.save.container, Container::Mkv, "MKV");
        });
        ui.add(
            egui::Slider::new(&mut app.cfg.save.warn_at_gb, 5..=500)
                .text("warn at folder size (GB)"),
        );
    });
}

fn hotkey(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Hotkeys", |ui| {
        if cfg!(windows) {
            ui.label("Global hotkeys, active while the SimpleClip daemon runs:");
        } else {
            ui.label("SimpleClip listens for these directly (needs input-group access on Linux):");
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Save clip:");
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.save, "hk-save");
        });
        ui.horizontal(|ui| {
            ui.label("Screenshot:");
            widgets::hotkey_field(ui, &mut app.cfg.hotkeys.screenshot, "hk-shot");
        });
        ui.colored_label(theme::MUTED, "Format: MOD+KEY, e.g. ALT+F10, CTRL+SHIFT+C");
    });
}

fn feedback(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Feedback and updates", |ui| {
        ui.checkbox(&mut app.cfg.general.notify, "Show a notification on save");
        ui.checkbox(&mut app.cfg.general.save_sound, "Play a sound on save");
        ui.checkbox(&mut app.cfg.general.update_check, "Check for updates");
    });
}

fn save_row(app: &mut ScApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.button("Save settings").clicked() {
            match config_io::save(&app.cfg) {
                Ok(()) => app.toast("settings saved", theme::OK),
                Err(e) => app.toast(e, theme::DANGER),
            }
        }
        if ui.button("Reload from disk").clicked() {
            app.cfg = config_io::load();
            app.toast("reloaded", theme::MUTED);
        }
    });
}
