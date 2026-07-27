use crate::app::{ScApp, Screen};
use crate::{theme, widgets};
use std::path::Path;

/// Scaffold for the Phase 6 trim view. The layout and controls are here; the
/// actual keyframe stream-copy (and optional re-encode) runs through the daemon
/// and lands with Phase 6, so the Export button is intentionally inert for now.
pub fn view(app: &mut ScApp, ui: &mut egui::Ui, path: &Path) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("\u{2039} Back to gallery").clicked() {
            app.screen = Screen::Gallery;
        }
        ui.heading("Trim");
    });
    ui.colored_label(theme::MUTED, path.display().to_string());
    ui.separator();

    widgets::card(ui, "Preview", |ui| {
        ui.colored_label(
            theme::MUTED,
            "Frame preview arrives in Phase 6 (ffmpeg-driven scrubber).",
        );
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 220.0),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
    });
    ui.add_space(10.0);
    trim_controls(app, ui);
}

fn trim_controls(app: &mut ScApp, ui: &mut egui::Ui) {
    widgets::card(ui, "In / out points", |ui| {
        let mut start = 0.0f32;
        let mut end = 100.0f32;
        let mut frame_accurate = false;
        ui.add(egui::Slider::new(&mut start, 0.0..=100.0).text("start %"));
        ui.add(egui::Slider::new(&mut end, 0.0..=100.0).text("end %"));
        ui.add_space(8.0);
        ui.checkbox(&mut frame_accurate, "Frame-accurate (re-encodes; slower)");
        ui.add_space(8.0);
        if ui.button("Export trimmed clip").clicked() {
            app.toast("trim export lands in Phase 6", theme::MUTED);
        }
    });
}
