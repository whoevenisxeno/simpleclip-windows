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
