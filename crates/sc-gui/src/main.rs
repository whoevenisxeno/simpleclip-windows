#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config_io;
mod daemon;
mod dialogs;
mod library;
#[cfg(windows)]
mod player;
mod screens;
mod star;
mod theme;
mod themes;
mod thumbs;
#[cfg(windows)]
mod update;
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

/// Only one SimpleClip window at a time: the first instance keeps an exclusive lock
/// file open for as long as it runs, and a second one exits straight away.
/// Returns `false` when another window already holds the lock.
#[cfg(windows)]
fn claim_single_instance() -> (bool, Option<std::fs::File>) {
    use std::os::windows::fs::OpenOptionsExt;
    let Some(dirs) = directories::ProjectDirs::from("", "", "SimpleClip") else {
        return (true, None);
    };
    let dir = dirs.data_local_dir().to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(dir.join("gui.lock"))
    {
        Ok(f) => (true, Some(f)),
        // Sharing violation: another window has it. Any other error: carry on.
        Err(e) if e.raw_os_error() == Some(32) => (false, None),
        Err(_) => (true, None),
    }
}

fn main() -> eframe::Result<()> {
    // (The design-review run opens its own windows and must not be blocked.)
    #[cfg(windows)]
    let _instance = if std::env::var_os("SC_GUI_SHOTS").is_some() {
        None
    } else {
        let (first, lock) = claim_single_instance();
        if !first {
            return Ok(());
        }
        lock
    };
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
