//! Loads gallery previews off the UI thread. Videos get their preview frame from
//! the daemon (which owns FFmpeg); screenshots are read directly. Textures are
//! created on the UI thread as results arrive.
//!
//! Loads can fail transiently (the daemon is still starting, a clip is still
//! being written), so failures are retried with a growing delay instead of being
//! remembered forever, and the UI is told to repaint so retries and queued loads
//! actually happen without the user moving the mouse.

use crate::daemon;
use sc_core::ipc::{Request, Response};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Preview width requested from the daemon, in pixels (shown at ~half this on
/// a 2x display).
pub const WIDTH: u32 = 640;
/// How many previews load at once.
const MAX_IN_FLIGHT: usize = 3;
/// Attempts before giving up on a preview.
const MAX_TRIES: u8 = 6;

enum State {
    Loading,
    Ready(egui::TextureHandle),
    Failed { at: Instant, tries: u8 },
}

type Done = (PathBuf, Option<egui::ColorImage>);

pub struct Thumbs {
    map: HashMap<PathBuf, State>,
    tries: HashMap<PathBuf, u8>,
    tx: crossbeam_channel::Sender<Done>,
    rx: crossbeam_channel::Receiver<Done>,
    in_flight: usize,
    ctx: egui::Context,
}

/// 1.5 s, 3 s, 6 s, ... capped at 20 s.
fn backoff(tries: u8) -> Duration {
    Duration::from_millis((1500u64 << tries.saturating_sub(1).min(4)).min(20_000))
}

impl Thumbs {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            map: HashMap::new(),
            tries: HashMap::new(),
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
            let tries = self.tries.get(&path).copied().unwrap_or(0) + 1;
            let state = match img {
                Some(img) => {
                    self.tries.remove(&path);
                    State::Ready(self.ctx.load_texture(
                        path.display().to_string(),
                        img,
                        egui::TextureOptions::LINEAR,
                    ))
                }
                None => {
                    self.tries.insert(path.clone(), tries);
                    State::Failed {
                        at: Instant::now(),
                        tries,
                    }
                }
            };
            self.map.insert(path, state);
        }
    }

    /// The preview texture for `path`, starting (or retrying) a background load
    /// when needed. `None` while loading or if it can't be made.
    pub fn get(&mut self, path: &Path, is_image: bool) -> Option<&egui::TextureHandle> {
        let start = match self.map.get(path) {
            None => true,
            Some(State::Loading) | Some(State::Ready(_)) => false,
            Some(State::Failed { at, tries }) => {
                if *tries >= MAX_TRIES {
                    false
                } else if at.elapsed() >= backoff(*tries) {
                    true
                } else {
                    // Come back when the retry is due.
                    self.ctx
                        .request_repaint_after(backoff(*tries).saturating_sub(at.elapsed()));
                    false
                }
            }
        };
        if start {
            if self.in_flight >= MAX_IN_FLIGHT {
                // Queue is full; look again shortly rather than waiting for input.
                self.ctx.request_repaint_after(Duration::from_millis(120));
            } else {
                self.in_flight += 1;
                self.map.insert(path.to_path_buf(), State::Loading);
                let (tx, ctx, p) = (self.tx.clone(), self.ctx.clone(), path.to_path_buf());
                std::thread::spawn(move || {
                    // A panic in a decoder must still release the slot.
                    let img = std::panic::catch_unwind(|| load(&p, is_image)).unwrap_or(None);
                    let _ = tx.send((p, img));
                    ctx.request_repaint();
                });
            }
        }
        match self.map.get(path) {
            Some(State::Ready(t)) => Some(t),
            _ => None,
        }
    }

    /// Whether any preview is still loading or waiting to be retried.
    pub fn pending(&self) -> bool {
        self.map.values().any(|s| match s {
            State::Loading => true,
            State::Failed { tries, .. } => *tries < MAX_TRIES,
            State::Ready(_) => false,
        })
    }

    /// True once every attempt has failed (so the UI can say so).
    pub fn gave_up(&self, path: &Path) -> bool {
        matches!(self.map.get(path), Some(State::Failed { tries, .. }) if *tries >= MAX_TRIES)
    }

    /// Drop a cached preview (e.g. after the file is deleted).
    pub fn forget(&mut self, path: &Path) {
        self.map.remove(path);
        self.tries.remove(path);
    }
}

fn load(path: &Path, is_image: bool) -> Option<egui::ColorImage> {
    if is_image {
        return decode_png(path);
    }
    // The daemon may still be starting; a couple of quick retries here cover that.
    for attempt in 0..3 {
        if let Ok(Response::Thumbnail { path }) = daemon::request(Request::Thumbnail {
            path: path.to_path_buf(),
            width: WIDTH,
        }) {
            if let Some(img) = decode_png(&path) {
                return Some(img);
            }
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(400));
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_and_cap() {
        assert!(backoff(1) < backoff(2));
        assert!(backoff(5) <= Duration::from_secs(20));
        assert_eq!(backoff(6), backoff(5));
    }
}
