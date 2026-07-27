use crate::app::ScApp;
use crate::{config_io, theme, widgets};

/// Editing the same TOML the daemon hot-reloads. Saving here triggers the
/// daemon's reload path; no restart needed.
pub fn view(app: &mut ScApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.heading("Settings");
    ui.colored_label(theme::MUTED, config_io::path().display().to_string());
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        widgets::card(ui, "Buffer", |ui| {
            ui.add(
                egui::Slider::new(&mut app.cfg.buffer.replay_duration_secs, 5..=300)
                    .text("replay seconds"),
            );
            ui.add(
                egui::Slider::new(&mut app.cfg.buffer.ram_cap_mb, 256..=8192).text("RAM cap (MB)"),
            );
            let est = app.cfg.estimated_buffer_mb();
            let over = est > app.cfg.buffer.ram_cap_mb;
            ui.colored_label(
                if over { theme::DANGER } else { theme::MUTED },
                format!("estimated ~{est} MB"),
            );
        });
        ui.add_space(8.0);
        widgets::card(ui, "Quality", |ui| {
            ui.add(
                egui::Slider::new(&mut app.cfg.encode.bitrate_kbps, 4_000..=80_000)
                    .text("bitrate kbps"),
            );
            use sc_core::encode::Codec::*;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.cfg.encode.codec, H264, "H.264");
                ui.selectable_value(&mut app.cfg.encode.codec, Hevc, "HEVC");
                ui.selectable_value(&mut app.cfg.encode.codec, Av1, "AV1");
            });
        });
        ui.add_space(8.0);
        widgets::card(ui, "General", |ui| {
            ui.checkbox(&mut app.cfg.general.save_sound, "Play a sound on save");
            ui.checkbox(&mut app.cfg.general.update_check, "Check for updates");
            ui.checkbox(&mut app.cfg.audio.mic_enabled, "Capture microphone");
        });
        ui.add_space(12.0);
        save_row(app, ui);
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
