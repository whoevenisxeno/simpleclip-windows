//! A small in-app video player: picture drawn as an egui texture, sound played
//! through the default output device. Sound is the clock when there is any, so
//! picture and voice stay together; with no sound (or no output device) a wall
//! clock drives the picture.
//!
//! Seeking restarts the decoders at the new position. That is quick enough for
//! short clips and keeps the decode threads simple.

use crate::theme::{icon, pal_ui};
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{pos2, vec2, Align2, FontId, Rect, Rounding, Sense, Stroke, Ui};
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use sc_ffmpeg::{MediaInfo, VideoFrame, AUDIO_RATE};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Widest picture the decoder produces.
const MAX_WIDTH: u32 = 1280;

/// Feeds decoded audio chunks to the output device and counts what was played.
struct ChanSource {
    rx: Receiver<Vec<f32>>,
    buf: Vec<f32>,
    i: usize,
    played: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
}

impl Iterator for ChanSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.i >= self.buf.len() {
            match self.rx.try_recv() {
                Ok(chunk) => {
                    self.buf = chunk;
                    self.i = 0;
                }
                // Decoder is behind: play silence rather than stalling the device.
                Err(TryRecvError::Empty) => return Some(0.0),
                Err(TryRecvError::Disconnected) => {
                    self.done.store(true, Ordering::Relaxed);
                    return None;
                }
            }
        }
        let s = self.buf.get(self.i).copied();
        self.i += 1;
        self.played.fetch_add(1, Ordering::Relaxed);
        s
    }
}

impl Source for ChanSource {
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

/// One start of the decoders; dropping it stops them.
struct Run {
    stop: Arc<AtomicBool>,
    video: Receiver<VideoFrame>,
    sink: Option<Sink>,
    played: Arc<AtomicU64>,
    audio_done: Arc<AtomicBool>,
    /// Where in the clip this run began, in seconds.
    from: f64,
    began: Instant,
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
    /// Current position in seconds.
    pos: f64,
    playing: bool,
    muted: bool,
    run: Option<Run>,
    /// Next decoded picture that isn't due yet.
    pending: Option<VideoFrame>,
    tex: Option<egui::TextureHandle>,
    last_tick: Instant,
    /// Show the first picture of a paused seek even though no time passes.
    want_still: bool,
    /// Seconds being dragged to on the bar (applied on release).
    scrub: Option<f64>,
    out: Option<(OutputStream, OutputStreamHandle)>,
}

impl Player {
    /// Open `path` and start playing it from the beginning.
    pub fn open(path: &Path) -> Self {
        let info = sc_ffmpeg::media_info(path).map_err(|e| e.to_string());
        let mut p = Self {
            path: path.to_path_buf(),
            info,
            pos: 0.0,
            playing: false,
            muted: false,
            run: None,
            pending: None,
            tex: None,
            last_tick: Instant::now(),
            want_still: false,
            scrub: None,
            out: None,
        };
        if p.info.is_ok() {
            p.start_at(0.0, true);
        }
        p
    }

    fn duration(&self) -> f64 {
        self.info.as_ref().map(|i| i.duration).unwrap_or(0.0)
    }

    /// Restart decoding at `secs`, playing or paused on the first picture.
    fn start_at(&mut self, secs: f64, play: bool) {
        self.run = None;
        self.pending = None;
        let secs = secs.clamp(0.0, (self.duration() - 0.05).max(0.0));
        self.pos = secs;
        self.playing = play;
        self.want_still = !play;
        self.last_tick = Instant::now();

        let stop = Arc::new(AtomicBool::new(false));
        let video = sc_ffmpeg::spawn_video(&self.path, secs, MAX_WIDTH, stop.clone());
        let played = Arc::new(AtomicU64::new(0));
        let audio_done = Arc::new(AtomicBool::new(false));
        let has_audio = self.info.as_ref().map(|i| i.has_audio).unwrap_or(false);
        let mut sink = None;
        if has_audio {
            if self.out.is_none() {
                self.out = OutputStream::try_default().ok();
            }
            if let Some((_, handle)) = &self.out {
                if let Ok(s) = Sink::try_new(handle) {
                    if !play {
                        s.pause();
                    }
                    s.set_volume(if self.muted { 0.0 } else { 1.0 });
                    s.append(ChanSource {
                        rx: sc_ffmpeg::spawn_audio(&self.path, secs, stop.clone()),
                        buf: Vec::new(),
                        i: 0,
                        played: played.clone(),
                        done: audio_done.clone(),
                    });
                    sink = Some(s);
                }
            }
        }
        if sink.is_none() {
            // Nothing will report audio progress, so the wall clock drives.
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
        });
    }

    fn pause(&mut self) {
        self.playing = false;
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.pause();
        }
    }

    fn resume(&mut self) {
        let end = (self.duration() - 0.1).max(0.0);
        if self.run.is_none() || self.pos >= end {
            self.start_at(0.0, true);
            return;
        }
        self.playing = true;
        self.want_still = false;
        self.last_tick = Instant::now();
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.play();
        }
    }

    fn set_muted(&mut self, m: bool) {
        self.muted = m;
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.set_volume(if m { 0.0 } else { 1.0 });
        }
    }

    /// Advance the clock and pick the picture that should be showing.
    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        let Some(run) = self.run.as_ref() else {
            return;
        };

        if self.playing {
            let played = run.played.load(Ordering::Relaxed);
            let audio_live = !run.audio_done.load(Ordering::Relaxed);
            if audio_live && played > 0 {
                let t = run.from + played as f64 / (2.0 * AUDIO_RATE as f64);
                self.pos = self.pos.max(t);
            } else if audio_live && run.began.elapsed() < Duration::from_millis(1500) {
                // Waiting for the first sound; hold the picture.
            } else {
                self.pos += dt;
            }
        }

        // Latest picture whose time has come (or the first one after a paused seek).
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
        if closed && self.pending.is_none() && self.playing {
            // End of the clip.
            self.pos = self.duration();
            self.pause();
        }
        if self.playing {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    /// Draw the picture and its controls.
    pub fn show(&mut self, ui: &mut Ui) {
        let p = pal_ui(ui);
        if let Err(e) = &self.info {
            widgets::notice(ui, p.red, &format!("Couldn't open this clip: {e}"));
            return;
        }
        self.tick(ui.ctx());

        let (iw, ih) = self
            .info
            .as_ref()
            .map(|i| (i.width.max(1) as f32, i.height.max(1) as f32))
            .unwrap_or((16.0, 9.0));
        let w = ui.available_width().min(640.0);
        let size = vec2(w, (w * ih / iw).round().min(420.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        ui.painter()
            .rect_filled(rect, Rounding::same(10.0), egui::Color32::BLACK);
        match &self.tex {
            Some(tex) => {
                egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                    .fit_to_exact_size(size)
                    .rounding(Rounding::same(10.0))
                    .paint_at(ui, rect);
            }
            None => {
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Loading\u{2026}",
                    FontId::new(12.0, egui::FontFamily::Proportional),
                    p.text_3,
                );
            }
        }
        if resp.clicked() {
            if self.playing {
                self.pause();
            } else {
                self.resume();
            }
        }

        ui.add_space(8.0);
        let dur = self.duration().max(0.001);
        let mut seek_to = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (g, tip) = if self.playing {
                (icon::PAUSE, "Pause")
            } else {
                (icon::PLAY, "Play")
            };
            if widgets::icon_button(ui, g, tip, tip, false).clicked() {
                if self.playing {
                    self.pause();
                } else {
                    self.resume();
                }
            }
            let shown = self.scrub.unwrap_or(self.pos).clamp(0.0, dur);
            ui.label(
                egui::RichText::new(format!("{} / {}", clock(shown), clock(dur)))
                    .size(12.0)
                    .color(p.text_2),
            );
            let bar_w = (ui.available_width() - 44.0).max(60.0);
            if let Some(t) = bar(ui, bar_w, shown / dur, &mut self.scrub) {
                seek_to = Some(t * dur);
            }
            let (g, tip, fb) = if self.muted {
                (icon::MUTE, "Unmute", "Unmute")
            } else {
                (icon::VOLUME, "Mute", "Mute")
            };
            if widgets::icon_button(ui, g, fb, tip, false).clicked() {
                let m = !self.muted;
                self.set_muted(m);
            }
        });
        if let Some(t) = seek_to {
            let play = self.playing;
            self.start_at(t, play);
        }
    }
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

/// A thin seek bar. Dragging only moves the handle; the seek happens when the
/// mouse is released (returns the fraction 0..1 then).
fn bar(ui: &mut Ui, width: f32, frac: f64, scrub: &mut Option<f64>) -> Option<f64> {
    let p = pal_ui(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click_and_drag());
    let track = Rect::from_min_max(
        pos2(rect.left(), rect.center().y - 2.5),
        pos2(rect.right(), rect.center().y + 2.5),
    );
    let at = |x: f32| ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
    let mut out = None;
    if resp.dragged() || resp.drag_started() {
        if let Some(pt) = resp.interact_pointer_pos() {
            *scrub = Some(at(pt.x));
        }
    }
    if resp.drag_stopped() {
        out = scrub.take();
    } else if resp.clicked() {
        if let Some(pt) = resp.interact_pointer_pos() {
            *scrub = None;
            out = Some(at(pt.x));
        }
    }
    let f = scrub.unwrap_or(frac).clamp(0.0, 1.0) as f32;
    ui.painter()
        .rect_filled(track, Rounding::same(3.0), p.card_hi);
    let fill = Rect::from_min_max(
        track.min,
        pos2(track.left() + track.width() * f, track.max.y),
    );
    ui.painter()
        .rect_filled(fill, Rounding::same(3.0), p.accent);
    let c = pos2(track.left() + track.width() * f, rect.center().y);
    ui.painter().circle_filled(c, 6.5, egui::Color32::WHITE);
    ui.painter().circle_stroke(
        c,
        6.5,
        Stroke::new(1.0_f32, egui::Color32::from_black_alpha(60)),
    );
    out
}
