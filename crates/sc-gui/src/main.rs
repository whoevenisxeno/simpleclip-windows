#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config_io;
mod daemon;
mod screens;
mod theme;
mod widgets;

use app::ScApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([880.0, 620.0])
            .with_min_inner_size([640.0, 480.0])
            .with_title("SimpleClip"),
        ..Default::default()
    };
    eframe::run_native(
        "SimpleClip",
        options,
        Box::new(|cc| Ok(Box::new(ScApp::new(cc)))),
    )
}
