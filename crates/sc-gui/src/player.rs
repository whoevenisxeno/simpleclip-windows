use crate::theme::pal_ui;
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{pos2, vec2, Rect, Rounding, Sense, Ui};
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use sc_ffmpeg::{MediaInfo, VideoFrame, AUDIO_RATE};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_WIDTH: u32 = 960;
const AUDIO_LAG: f64 = 0.09;
const PREVIEW_HEIGHT: f32 = 330.0;

#[derive(Clone, Copy, Default, PartialEq)]
pub struct View {
    pub turn: u16,
    pub flip_h: bool,
    pub flip_v: bool,
}

struct Track {
    rx: Receiver<Vec<f32>>,
    buf: Vec<f32>,
    i: usize,
    ended: bool,
}

struct Mix {
    tracks: Vec<Track>,
    played: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
}

impl Iterator for Mix {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let mut sum = 0.0;
        let mut alive = false;
        for t in &mut self.tracks {
            if t.ended {
                continue;
            }
            if t.i >= t.buf.len() {
                match t.rx.try_recv() {
                    Ok(chunk) => {
                        t.buf = chunk;
                        t.i = 0;
                    }
                    Err(TryRecvError::Empty) => {
                        alive = true;
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => {
                        t.ended = true;
                        continue;
                    }
                }
            }
            alive = true;
            sum += t.buf.get(t.i).copied().unwrap_or(0.0);
            t.i += 1;
        }
        if !alive {
            self.done.store(true, Ordering::Relaxed);
            return None;
        }
        self.played.fetch_add(1, Ordering::Relaxed);
        Some(sum.clamp(-1.0, 1.0))
    }
}

impl Source for Mix {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        AUDIO_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Run {
    stop: Arc<AtomicBool>,
    video: Receiver<VideoFrame>,
    sink: Option<Sink>,
    played: Arc<AtomicU64>,
    audio_done: Arc<AtomicBool>,
    from: f64,
    began: Instant,
    shown: u32,
    last_played: u64,
    last_change: Instant,
}

impl Drop for Run {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.sink.take() {
            s.stop();
        }
    }
}

pub struct Player {
    path: PathBuf,
    info: Result<MediaInfo, String>,
    failed: Option<String>,
    pos: f64,
    playing: bool,
    gain: f32,
    run: Option<Run>,
    pending: Option<VideoFrame>,
    tex: Option<egui::TextureHandle>,
    last_tick: Instant,
    want_still: bool,
    out: Option<(OutputStream, OutputStreamHandle)>,
    segments: Vec<(f64, f64)>,
}

impl Player {
    pub fn open_paused(path: &Path, at: f64) -> Self {
        let mut p = Self::open(path);
        if p.info.is_ok() {
            p.start_at(at, false);
        }
        p
    }

    pub fn set_segments(&mut self, segments: Vec<(f64, f64)>) {
        self.segments = segments;
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(0.0, 2.0);
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.set_volume(self.gain);
        }
    }

    pub fn open(path: &Path) -> Self {
        let info = sc_ffmpeg::media_info(path).map_err(|e| e.to_string());
        let mut p = Self {
            path: path.to_path_buf(),
            info,
            failed: None,
            pos: 0.0,
            playing: false,
            gain: 1.0,
            run: None,
            pending: None,
            tex: None,
            last_tick: Instant::now(),
            want_still: false,
            out: None,
            segments: Vec::new(),
        };
        if p.info.is_ok() {
            p.start_at(0.0, true);
        }
        p
    }

    pub fn duration(&self) -> f64 {
        self.info.as_ref().map(|i| i.duration).unwrap_or(0.0)
    }

    pub fn size(&self) -> (u32, u32) {
        self.info
            .as_ref()
            .map(|i| (i.width, i.height))
            .unwrap_or((16, 9))
    }

    pub fn position(&self) -> f64 {
        self.pos
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn seek(&mut self, secs: f64) {
        let play = self.playing;
        self.start_at(secs, play);
    }

    fn start_at(&mut self, secs: f64, play: bool) {
        self.run = None;
        self.pending = None;
        self.failed = None;
        let secs = secs.clamp(0.0, (self.duration() - 0.05).max(0.0));
        self.pos = secs;
        self.playing = play;
        self.want_still = !play;
        self.last_tick = Instant::now();

        let stop = Arc::new(AtomicBool::new(false));
        let video = sc_ffmpeg::spawn_video(&self.path, secs, MAX_WIDTH, stop.clone());
        let played = Arc::new(AtomicU64::new(0));
        let audio_done = Arc::new(AtomicBool::new(false));
        let tracks = self.info.as_ref().map(|i| i.audio_streams).unwrap_or(0);
        let mut sink = None;
        if tracks > 0 {
            if self.out.is_none() {
                self.out = OutputStream::try_default().ok();
            }
            if let Some((_, handle)) = &self.out {
                if let Ok(s) = Sink::try_new(handle) {
                    if !play {
                        s.pause();
                    }
                    s.set_volume(self.gain);
                    s.append(Mix {
                        tracks: (0..tracks)
                            .map(|n| Track {
                                rx: sc_ffmpeg::spawn_audio(&self.path, secs, n, stop.clone()),
                                buf: Vec::new(),
                                i: 0,
                                ended: false,
                            })
                            .collect(),
                        played: played.clone(),
                        done: audio_done.clone(),
                    });
                    sink = Some(s);
                }
            }
        }
        if sink.is_none() {
            audio_done.store(true, Ordering::Relaxed);
        }
        self.run = Some(Run {
            stop,
            video,
            sink,
            played,
            audio_done,
            from: secs,
            began: Instant::now(),
            shown: 0,
            last_played: 0,
            last_change: Instant::now(),
        });
    }

    pub fn pause(&mut self) {
        self.playing = false;
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.pause();
        }
    }

    fn resume(&mut self) {
        if self.run.is_none() || self.failed.is_some() {
            self.start_at(self.pos, true);
            return;
        }
        self.playing = true;
        self.want_still = false;
        self.last_tick = Instant::now();
        if let Some(r) = self.run.as_mut() {
            r.last_change = Instant::now();
            if let Some(s) = r.sink.as_ref() {
                s.play();
            }
        }
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.resume();
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        let looping = !self.segments.is_empty();
        if self.playing && looping {
            let inside = self
                .segments
                .iter()
                .any(|s| self.pos >= s.0 && self.pos < s.1 - 0.02);
            if !inside {
                let to = self
                    .segments
                    .iter()
                    .map(|s| s.0)
                    .find(|s| *s > self.pos)
                    .unwrap_or(self.segments[0].0);
                self.start_at(to, true);
            }
        }
        let Some(run) = self.run.as_mut() else {
            return;
        };

        if self.playing {
            let played = run.played.load(Ordering::Relaxed);
            if played != run.last_played {
                run.last_played = played;
                run.last_change = now;
            }
            let stalled = now.duration_since(run.last_change) > Duration::from_millis(700);
            let audio_live = !run.audio_done.load(Ordering::Relaxed) && !stalled;
            if audio_live && played > 0 {
                let t = run.from + played as f64 / (2.0 * AUDIO_RATE as f64) - AUDIO_LAG;
                self.pos = self.pos.max(t);
            } else if audio_live && run.began.elapsed() < Duration::from_millis(1500) {
            } else {
                self.pos += dt;
            }
        }

        let mut show: Option<VideoFrame> = None;
        let mut closed = false;
        loop {
            let next = match self.pending.take() {
                Some(f) => f,
                None => match run.video.try_recv() {
                    Ok(f) => f,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                },
            };
            if self.want_still || next.pts <= self.pos + 0.004 {
                self.want_still = false;
                show = Some(next);
            } else {
                self.pending = Some(next);
                break;
            }
        }
        if let Some(f) = show {
            run.shown += 1;
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [f.width as usize, f.height as usize],
                &f.rgba,
            );
            match &mut self.tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    self.tex = Some(ctx.load_texture("player", img, egui::TextureOptions::LINEAR))
                }
            }
        }
        if closed && self.pending.is_none() {
            if run.shown == 0 {
                self.failed = Some("this clip could not be decoded".into());
                self.pause();
            } else if self.playing {
                if looping {
                    self.pos = self.pos.max(self.duration());
                } else {
                    self.pos = self.duration();
                    self.pause();
                }
            }
        }
        if self.playing || self.want_still {
            ctx.request_repaint();
        }
    }

    pub fn frame(&mut self, ui: &mut Ui, view: View) -> Option<(Rect, egui::Response)> {
        let p = pal_ui(ui);
        if let Err(e) = &self.info {
            widgets::notice(ui, p.red, &format!("Couldn’t open this clip: {e}"));
            return None;
        }
        self.tick(ui.ctx());

        let (iw, ih) = self.size();
        let (iw, ih) = if view.turn % 180 == 90 {
            (ih as f32, iw as f32)
        } else {
            (iw.max(1) as f32, ih.max(1) as f32)
        };
        let avail = ui.available_width();
        let w = avail.min(PREVIEW_HEIGHT * iw / ih).min(820.0);
        let size = vec2(w, (w * ih / iw).round());
        let pad = ((avail - w) / 2.0).max(0.0);
        let mut out = None;
        ui.horizontal(|ui| {
            ui.add_space(pad);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            self.paint(ui, rect, view);
            out = Some((rect, resp));
        });
        out
    }

    fn paint(&self, ui: &Ui, rect: Rect, view: View) {
        let p = pal_ui(ui);
        ui.painter()
            .rect_filled(rect, Rounding::same(10.0), egui::Color32::BLACK);
        let Some(tex) = &self.tex else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.failed.as_deref().unwrap_or("Loading\u{2026}"),
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
                p.text_3,
            );
            return;
        };
        let swapped = view.turn % 180 == 90;
        let upright = if swapped {
            Rect::from_center_size(rect.center(), vec2(rect.height(), rect.width()))
        } else {
            rect
        };
        let (u0, u1) = if view.flip_h { (1.0, 0.0) } else { (0.0, 1.0) };
        let (v0, v1) = if view.flip_v { (1.0, 0.0) } else { (0.0, 1.0) };
        let mut img = egui::Image::new(egui::load::SizedTexture::new(tex.id(), upright.size()))
            .fit_to_exact_size(upright.size())
            .uv(Rect::from_min_max(pos2(u0, v0), pos2(u1, v1)));
        if view.turn == 0 {
            img = img.rounding(Rounding::same(10.0));
        } else {
            img = img.rotate((view.turn as f32).to_radians(), vec2(0.5, 0.5));
        }
        img.paint_at(ui, upright);
    }
}
