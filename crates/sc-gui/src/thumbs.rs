//! Loads gallery previews off the UI thread. Videos get their preview frame from
//! the daemon (which owns FFmpeg); screenshots are read directly. Textures are
//! created on the UI thread as results arrive.

use crate::daemon;
use sc_core::ipc::{Request, Response};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Preview width requested from the daemon, in pixels (shown at ~half this on
/// a 2x display).
pub const WIDTH: u32 = 640;
/// How many previews load at once.
const MAX_IN_FLIGHT: usize = 3;

enum State {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

pub struct Thumbs {
    map: HashMap<PathBuf, State>,
    tx: crossbeam_channel::Sender<(PathBuf, Option<egui::ColorImage>)>,
    rx: crossbeam_channel::Receiver<(PathBuf, Option<egui::ColorImage>)>,
    in_flight: usize,
    ctx: egui::Context,
}

impl Thumbs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            map: HashMap::new(),
            tx,
            rx,
            in_flight: 0,
            ctx,
        }
    }

    /// Turn finished loads into textures. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok((path, img)) = self.rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            let state = match img {
                Some(img) => State::Ready(self.ctx.load_texture(
                    path.display().to_string(),
                    img,
                    egui::TextureOptions::LINEAR,
                )),
                None => State::Failed,
            };
            self.map.insert(path, state);
        }
    }

    /// The preview texture for `path`, starting a background load the first time
    /// it is asked for. `None` while loading or if it can't be made.
    pub fn get(&mut self, path: &Path, is_image: bool) -> Option<&egui::TextureHandle> {
        if !self.map.contains_key(path) {
            if self.in_flight >= MAX_IN_FLIGHT {
                return None; // asked again next frame
            }
            self.in_flight += 1;
            self.map.insert(path.to_path_buf(), State::Loading);
            let (tx, ctx, p) = (self.tx.clone(), self.ctx.clone(), path.to_path_buf());
            std::thread::spawn(move || {
                let img = load(&p, is_image);
                let _ = tx.send((p, img));
                ctx.request_repaint();
            });
        }
        match self.map.get(path) {
            Some(State::Ready(t)) => Some(t),
            _ => None,
        }
    }

    /// Drop a cached preview (e.g. after the file is deleted).
    pub fn forget(&mut self, path: &Path) {
        self.map.remove(path);
    }
}

fn load(path: &Path, is_image: bool) -> Option<egui::ColorImage> {
    if is_image {
        return decode_png(path);
    }
    match daemon::request(Request::Thumbnail {
        path: path.to_path_buf(),
        width: WIDTH,
    }) {
        Ok(Response::Thumbnail { path }) => decode_png(&path),
        _ => None,
    }
}

/// Decode a PNG into an egui image, shrinking big ones (screenshots) so the
/// gallery doesn't hold full-resolution textures.
fn decode_png(path: &Path) -> Option<egui::ColorImage> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return None,
    };
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    // Nearest-neighbour downscale to the preview width.
    let step = (w / WIDTH as usize).max(1);
    let (tw, th) = (w / step, h / step);
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        for x in 0..tw {
            let i = ((y * step) * w + x * step) * channels;
            rgba.extend_from_slice(&buf[i..i + 3]);
            rgba.push(if channels == 4 { buf[i + 3] } else { 255 });
        }
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([tw, th], &rgba))
}
