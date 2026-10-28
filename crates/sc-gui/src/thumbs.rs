use crate::daemon;
use sc_core::ipc::{Request, Response};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const WIDTH: u32 = 480;
const MAX_TEXTURES: usize = 36;
const MAX_IN_FLIGHT: usize = 3;
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
    frame: u64,
    used: HashMap<PathBuf, u64>,
}

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
            frame: 0,
            used: HashMap::new(),
        }
    }

    pub fn poll(&mut self) {
        self.frame += 1;
        if self.frame & 127 == 0 {
            self.trim_to(MAX_TEXTURES);
        }
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

    pub fn get(&mut self, path: &Path, is_image: bool) -> Option<&egui::TextureHandle> {
        self.used.insert(path.to_path_buf(), self.frame);
        let start = match self.map.get(path) {
            None => true,
            Some(State::Loading) | Some(State::Ready(_)) => false,
            Some(State::Failed { at, tries }) => {
                let wait = if *tries >= MAX_TRIES {
                    Duration::from_secs(20)
                } else {
                    backoff(*tries)
                };
                if at.elapsed() >= wait {
                    true
                } else {
                    self.ctx
                        .request_repaint_after(wait.saturating_sub(at.elapsed()));
                    false
                }
            }
        };
        if start {
            if self.in_flight >= MAX_IN_FLIGHT {
                self.ctx.request_repaint_after(Duration::from_millis(120));
            } else {
                self.in_flight += 1;
                self.map.insert(path.to_path_buf(), State::Loading);
                let (tx, ctx, p) = (self.tx.clone(), self.ctx.clone(), path.to_path_buf());
                std::thread::spawn(move || {
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

    pub fn pending(&self) -> bool {
        self.map.values().any(|s| match s {
            State::Loading => true,
            State::Failed { tries, .. } => *tries < MAX_TRIES,
            State::Ready(_) => false,
        })
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let mut s = (0, 0, 0);
        for st in self.map.values() {
            match st {
                State::Ready(_) => s.0 += 1,
                State::Failed { tries, .. } if *tries >= MAX_TRIES => s.1 += 1,
                _ => s.2 += 1,
            }
        }
        s
    }

    pub fn gave_up(&self, path: &Path) -> bool {
        matches!(self.map.get(path), Some(State::Failed { tries, .. }) if *tries >= MAX_TRIES)
    }

    pub fn retry_failed(&mut self) {
        let failed: Vec<PathBuf> = self
            .map
            .iter()
            .filter(|(_, s)| matches!(s, State::Failed { .. }))
            .map(|(p, _)| p.clone())
            .collect();
        for p in failed {
            self.map.remove(&p);
            self.tries.remove(&p);
        }
    }

    pub fn trim_to(&mut self, keep: usize) {
        let mut ready: Vec<(u64, PathBuf)> = self
            .map
            .iter()
            .filter(|(_, s)| matches!(s, State::Ready(_)))
            .map(|(p, _)| (self.used.get(p).copied().unwrap_or(0), p.clone()))
            .collect();
        if ready.len() <= keep {
            return;
        }
        ready.sort();
        let drop = ready.len() - keep;
        for (_, p) in ready.into_iter().take(drop) {
            self.map.remove(&p);
            self.used.remove(&p);
        }
    }

    pub fn forget(&mut self, path: &Path) {
        self.map.remove(path);
        self.used.remove(path);
        self.tries.remove(path);
    }
}

fn load(path: &Path, is_image: bool) -> Option<egui::ColorImage> {
    if is_image {
        return decode_png(path);
    }
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

pub fn decode_png(path: &Path) -> Option<egui::ColorImage> {
    decode_png_bytes(&std::fs::read(path).ok()?)
}

pub fn decode_png_bytes(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
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
