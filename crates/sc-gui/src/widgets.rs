use crate::theme;
use egui::{Color32, RichText};
use sc_core::capture::CaptureState;

pub fn state_badge(state: CaptureState) -> (&'static str, Color32) {
    match state {
        CaptureState::Active => ("capturing", theme::OK),
        CaptureState::Paused => ("paused", theme::WARN),
        CaptureState::NeedsConsent => ("needs consent", theme::WARN),
        CaptureState::Stopped => ("stopped", theme::MUTED),
    }
}

pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let bg = color.linear_multiply(0.18);
    egui::Frame::none()
        .fill(bg)
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(12.0).strong());
        });
}

/// A hotkey text field with a "Record" button: click it, press the combination,
/// and the field is filled in as `CTRL+ALT+F10`. Typing by hand still works.
pub fn hotkey_field(ui: &mut egui::Ui, value: &mut String, id: &str) {
    let rec_id = ui.make_persistent_id(id);
    let recording = ui.data(|d| d.get_temp::<bool>(rec_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.add_enabled(
            !recording,
            egui::TextEdit::singleline(value).desired_width(150.0),
        );
        if !recording {
            if ui.button("Record").clicked() {
                ui.data_mut(|d| d.insert_temp(rec_id, true));
            }
            return;
        }
        ui.colored_label(theme::WARN, "press the combination (Esc cancels)");
        let events = ui.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = ev
            else {
                continue;
            };
            if key != egui::Key::Escape {
                let mut parts = Vec::new();
                if modifiers.ctrl {
                    parts.push("CTRL".to_string());
                }
                if modifiers.alt {
                    parts.push("ALT".to_string());
                }
                if modifiers.shift {
                    parts.push("SHIFT".to_string());
                }
                parts.push(key.name().to_ascii_uppercase());
                *value = parts.join("+");
            }
            ui.data_mut(|d| d.insert_temp(rec_id, false));
        }
    });
}

/// A titled card for grouping wizard/settings fields.
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(ui.visuals().faint_bg_color)
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong().size(15.0));
            ui.add_space(8.0);
            add(ui)
        })
        .inner
}
