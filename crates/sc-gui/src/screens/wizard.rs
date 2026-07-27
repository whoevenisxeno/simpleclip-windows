use crate::app::{ScApp, Screen};
use crate::{config_io, daemon, theme, widgets};
use sc_core::audio::AudioDevice;
use sc_core::capture::MonitorInfo;
use sc_core::ipc::{Request, Response};

const STEPS: &[&str] = &[
    "Welcome",
    "Monitor",
    "Microphone",
    "Replay",
    "Save location",
    "Hotkeys",
    "Quality",
];

#[derive(Default)]
pub struct WizardState {
    pub step: usize,
    pub monitors: Vec<MonitorInfo>,
    pub audio: Vec<AudioDevice>,
    fetched: bool,
}

fn fetch_devices(w: &mut WizardState) {
    if w.fetched {
        return;
    }
    if let Ok(Response::Monitors(m)) = daemon::request(Request::ListMonitors) {
        w.monitors = m;
    }
    if let Ok(Response::AudioDevices(a)) = daemon::request(Request::ListAudioDevices) {
        w.audio = a;
    }
    w.fetched = true;
}

pub fn view(app: &mut ScApp, ui: &mut egui::Ui) {
    fetch_devices(&mut app.wizard);
    stepper(app, ui);
    ui.separator();
    ui.add_space(6.0);
    egui::ScrollArea::vertical().show(ui, |ui| match app.wizard.step {
        0 => step_welcome(ui),
        1 => step_monitor(app, ui),
        2 => step_mic(app, ui),
        3 => step_replay(app, ui),
        4 => step_save(app, ui),
        5 => step_hotkeys(app, ui),
        _ => step_quality(app, ui),
    });
    ui.add_space(8.0);
    nav_buttons(app, ui);
}

fn stepper(app: &ScApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        for (i, name) in STEPS.iter().enumerate() {
            let color = if i == app.wizard.step {
                theme::ACCENT
            } else if i < app.wizard.step {
                theme::OK
            } else {
                theme::MUTED
            };
            ui.colored_label(color, format!("{}\u{2009}{name}", i + 1));
            if i + 1 < STEPS.len() {
                ui.colored_label(theme::MUTED, "\u{203A}");
            }
        }
    });
}

fn nav_buttons(app: &mut ScApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if app.wizard.step > 0 && ui.button("\u{2039} Back").clicked() {
            app.wizard.step -= 1;
        }
        let last = app.wizard.step == STEPS.len() - 1;
        let label = if last { "Finish" } else { "Next \u{203A}" };
        if ui.button(label).clicked() {
            if last {
                finish(app);
            } else {
                app.wizard.step += 1;
            }
        }
    });
}

fn finish(app: &mut ScApp) {
    match config_io::save(&app.cfg) {
        Ok(()) => {
            app.toast("setup saved - SimpleClip is configured", theme::OK);
            app.screen = Screen::Dashboard;
        }
        Err(e) => app.toast(format!("cannot save config: {e}"), theme::DANGER),
    }
}

fn step_welcome(ui: &mut egui::Ui) {
    widgets::card(ui, "Welcome to SimpleClip", |ui| {
        ui.label(
            "SimpleClip keeps the last few seconds of your screen buffered at all times. \
             When something worth keeping happens, you hit one hotkey and it writes the clip \
             - no record button, no interruption.",
        );
        ui.add_space(6.0);
        ui.label("This quick setup picks your monitor, audio, buffer length, and hotkey.");
    });
}

fn step_monitor(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Which monitor should SimpleClip capture?", |ui| {
        if app.wizard.monitors.is_empty() {
            ui.colored_label(
                theme::MUTED,
                "No monitors reported yet - capture detection lands in Phase 1. \
                 You can still finish setup; the daemon will pick the primary display.",
            );
        }
        for m in app.wizard.monitors.clone() {
            let label = format!(
                "{}  ({}\u{00D7}{} @ {} Hz)",
                m.name,
                m.width,
                m.height,
                m.refresh_mhz / 1000
            );
            if ui
                .selectable_label(app.cfg.capture.monitor_id == m.id, label)
                .clicked()
            {
                app.cfg.capture.monitor_id = m.id.clone();
            }
        }
        ui.add_space(8.0);
        ui.checkbox(&mut app.cfg.capture.show_cursor, "Capture the mouse cursor");
    });
}

fn step_mic(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Microphone", |ui| {
        ui.label("Desktop/system audio is always captured. Add your mic too?");
        ui.add_space(6.0);
        ui.checkbox(&mut app.cfg.audio.mic_enabled, "Include microphone");
        if app.cfg.audio.mic_enabled {
            ui.add_space(6.0);
            let mics: Vec<_> = app
                .wizard
                .audio
                .iter()
                .filter(|d| !d.is_monitor)
                .cloned()
                .collect();
            if mics.is_empty() {
                ui.colored_label(theme::MUTED, "No input devices reported yet (Phase 1).");
            }
            for d in mics {
                let sel = app.cfg.audio.mic_device.as_deref() == Some(d.id.as_str());
                if ui.selectable_label(sel, &d.name).clicked() {
                    app.cfg.audio.mic_device = Some(d.id.clone());
                }
            }
            ui.add_space(6.0);
            ui.radio_value(
                &mut app.cfg.audio.tracks,
                sc_core::config::AudioTracks::Mixed,
                "Mix into one track (most compatible)",
            );
            ui.radio_value(
                &mut app.cfg.audio.tracks,
                sc_core::config::AudioTracks::Separate,
                "Keep desktop + mic on separate tracks (MKV)",
            );
        }
    });
}

fn step_replay(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "How much should SimpleClip keep?", |ui| {
        ui.label("Replay buffer length. Longer means more RAM used.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for secs in [15u32, 30, 60, 120, 300] {
                let label = if secs < 60 {
                    format!("{secs}s")
                } else {
                    format!("{}m", secs / 60)
                };
                if ui
                    .selectable_label(app.cfg.buffer.replay_duration_secs == secs, label)
                    .clicked()
                {
                    app.cfg.buffer.replay_duration_secs = secs;
                }
            }
        });
        ui.add_space(10.0);
        let est = app.cfg.estimated_buffer_mb();
        let over = est > app.cfg.buffer.ram_cap_mb;
        let color = if over { theme::DANGER } else { theme::OK };
        ui.colored_label(
            color,
            format!(
                "Estimated buffer RAM: ~{est} MB (cap {} MB)",
                app.cfg.buffer.ram_cap_mb
            ),
        );
        ui.add(egui::Slider::new(&mut app.cfg.buffer.ram_cap_mb, 256..=8192).text("RAM cap (MB)"));
        if over {
            ui.colored_label(
                theme::DANGER,
                "Over the cap - lower the duration or raise the cap to continue.",
            );
        }
    });
}

fn step_save(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Where should clips go?", |ui| {
        let mut dir = app
            .cfg
            .save
            .directory
            .clone()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        ui.label("Save folder (blank = your Videos folder):");
        if ui.text_edit_singleline(&mut dir).changed() {
            app.cfg.save.directory = if dir.trim().is_empty() {
                None
            } else {
                Some(dir.trim().into())
            };
        }
        ui.add_space(8.0);
        ui.label("Organize clips into subfolders:");
        use sc_core::config::FolderPolicy::*;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut app.cfg.save.folder_policy, PerDay, "Per day");
            ui.selectable_value(&mut app.cfg.save.folder_policy, PerApp, "Per game/app");
            ui.selectable_value(&mut app.cfg.save.folder_policy, Flat, "All in one folder");
        });
        ui.add_space(8.0);
        use sc_core::config::Container::*;
        ui.horizontal(|ui| {
            ui.label("Container:");
            ui.selectable_value(&mut app.cfg.save.container, Mp4, "MP4 (shareable)");
            ui.selectable_value(&mut app.cfg.save.container, Mkv, "MKV (crash-safe)");
        });
    });
}

fn step_hotkeys(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Save hotkey", |ui| {
        if cfg!(target_os = "linux") {
            ui.label(
                "On Wayland the most reliable hotkey is a compositor bind that runs the CLI. \
                 Add these to your compositor config - SimpleClip's daemon does the rest:",
            );
            ui.add_space(8.0);
            let hypr = "bind = SUPER, F10, exec, sc save\nbind = SUPER, F11, exec, sc screenshot";
            ui.code(hypr);
            if ui.button("Copy Hyprland binds").clicked() {
                ui.output_mut(|o| o.copied_text = hypr.to_string());
                app.toast("copied to clipboard", theme::OK);
            }
            ui.add_space(6.0);
            ui.colored_label(
                theme::MUTED,
                "niri and other compositors: bind the same two commands.",
            );
        } else {
            ui.label("Pick global hotkeys (registered by SimpleClip):");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Save clip:");
                ui.text_edit_singleline(&mut app.cfg.hotkeys.save);
            });
            ui.horizontal(|ui| {
                ui.label("Screenshot:");
                ui.text_edit_singleline(&mut app.cfg.hotkeys.screenshot);
            });
        }
    });
}

fn step_quality(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "Quality", |ui| {
        ui.label("Video bitrate. Higher looks better but uses more RAM and disk.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (name, kbps) in [
                ("Low", 8_000u32),
                ("Balanced", 20_000),
                ("High", 40_000),
                ("Ultra", 80_000),
            ] {
                if ui
                    .selectable_label(app.cfg.encode.bitrate_kbps == kbps, name)
                    .clicked()
                {
                    app.cfg.encode.bitrate_kbps = kbps;
                }
            }
        });
        ui.add_space(10.0);
        use sc_core::encode::Codec::*;
        ui.horizontal(|ui| {
            ui.label("Codec:");
            ui.selectable_value(&mut app.cfg.encode.codec, H264, "H.264");
            ui.selectable_value(&mut app.cfg.encode.codec, Hevc, "HEVC");
            ui.selectable_value(&mut app.cfg.encode.codec, Av1, "AV1");
        });
        if app.cfg.encode.codec != H264 {
            ui.colored_label(
                theme::WARN,
                "HEVC/AV1 may not upload or play everywhere. H.264 is safest.",
            );
        }
        ui.add_space(8.0);
        ui.colored_label(
            theme::MUTED,
            format!(
                "Estimated buffer RAM at these settings: ~{} MB",
                app.cfg.estimated_buffer_mb()
            ),
        );
    });
}
