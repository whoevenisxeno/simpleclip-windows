#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config_io;
mod daemon;
mod dialogs;
mod library;
#[cfg(windows)]
mod player;
mod screens;
mod theme;
mod thumbs;
mod widgets;

use app::ScApp;

/// The app icon, decoded for the window/taskbar (the .exe's own icon is embedded
/// at build time by build.rs).
fn window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/icon-256.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1080.0, 720.0])
        .with_min_inner_size([820.0, 560.0])
        .with_title("SimpleClip");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "SimpleClip",
        options,
        Box::new(|cc| Ok(Box::new(ScApp::new(cc)))),
    )
}
