use crate::theme::{icon, pal_ui};
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{pos2, vec2, Align2, FontId, Key, Rect, Rounding, Sense, Stroke, Ui};
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use sc_ffmpeg::{MediaInfo, VideoFrame, AUDIO_RATE};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_WIDTH: u32 = 960;
const AUDIO_LAG: f64 = 0.09;
const SKIP: f64 = 5.0;

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
    muted: bool,
    run: Option<Run>,
    pending: Option<VideoFrame>,
    tex: Option<egui::TextureHandle>,
    last_tick: Instant,
    want_still: bool,
    scrub: Option<f64>,
    out: Option<(OutputStream, OutputStreamHandle)>,
    range: Option<(f64, f64)>,
    focus: Option<(f64, Instant)>,
}

impl Player {
    pub fn open_paused(path: &Path, at: f64) -> Self {
        let mut p = Self::open(path);
        if p.info.is_ok() {
            p.start_at(at, false);
        }
        p
    }

    pub fn set_range(&mut self, start: f64, end: f64) {
        self.range = Some((start, end.max(start + 0.1)));
    }

    pub fn focus(&mut self, secs: f64) {
        self.pause();
        self.focus = Some((secs, Instant::now()));
    }

    pub fn open(path: &Path) -> Self {
        let info = sc_ffmpeg::media_info(path).map_err(|e| e.to_string());
        let mut p = Self {
            path: path.to_path_buf(),
            info,
            failed: None,
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
            range: None,
            focus: None,
        };
        if p.info.is_ok() {
            p.start_at(0.0, true);
        }
        p
    }

    fn duration(&self) -> f64 {
        self.info.as_ref().map(|i| i.duration).unwrap_or(0.0)
    }

    fn span(&self) -> (f64, f64) {
        match self.range {
            Some((s, e)) => (s, e.min(self.duration().max(s + 0.1))),
            None => (0.0, self.duration()),
        }
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
                    s.set_volume(if self.muted { 0.0 } else { 1.0 });
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

    fn pause(&mut self) {
        self.playing = false;
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.pause();
        }
    }

    fn resume(&mut self) {
        let (lo, hi) = self.span();
        let end = (hi - 0.1).max(lo);
        if self.run.is_none() || self.pos >= end || self.pos < lo - 0.05 || self.failed.is_some() {
            self.start_at(lo, true);
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

    fn skip(&mut self, secs: f64) {
        let play = self.playing;
        let (lo, hi) = self.span();
        let to = (self.pos + secs).clamp(lo, (hi - 0.1).max(lo));
        self.start_at(to, play);
    }

    fn set_muted(&mut self, m: bool) {
        self.muted = m;
        if let Some(s) = self.run.as_ref().and_then(|r| r.sink.as_ref()) {
            s.set_volume(if m { 0.0 } else { 1.0 });
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(0.25);
        self.last_tick = now;
        if let Some((t, at)) = self.focus {
            if at.elapsed() >= Duration::from_millis(90) {
                self.focus = None;
                self.start_at(t, false);
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        let (lo, hi) = self.span();
        let looping = self.range.is_some();
        if self.playing && self.focus.is_none() && self.pos >= hi - 0.02 && looping {
            self.start_at(lo, true);
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
                    self.pos = self.pos.max(hi);
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

    fn keys(&mut self, ui: &Ui) {
        let ctx = ui.ctx();
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (space, left, right) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
            )
        });
        if space {
            self.toggle();
        }
        if left {
            self.skip(-SKIP);
        }
        if right {
            self.skip(SKIP);
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let p = pal_ui(ui);
        if let Err(e) = &self.info {
            widgets::notice(ui, p.red, &format!("Couldn’t open this clip: {e}"));
            return;
        }
        self.keys(ui);
        self.tick(ui.ctx());

        let (iw, ih) = self
            .info
            .as_ref()
            .map(|i| (i.width.max(1) as f32, i.height.max(1) as f32))
            .unwrap_or((16.0, 9.0));
        let avail = ui.available_width();
        let h_cap = 420.0;
        let w = avail.min(h_cap * iw / ih).min(820.0);
        let size = vec2(w, (w * ih / iw).round());
        let pad = ((avail - w) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.vertical(|ui| {
                ui.set_width(w);
                self.draw(ui, size);
            });
        });
    }

    fn draw(&mut self, ui: &mut Ui, size: egui::Vec2) {
        let p = pal_ui(ui);
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
                    self.failed.as_deref().unwrap_or("Loading\u{2026}"),
                    FontId::new(12.0, egui::FontFamily::Proportional),
                    p.text_3,
                );
            }
        }
        if resp.clicked() {
            self.toggle();
        }

        ui.add_space(8.0);
        let (lo, hi) = self.span();
        let dur = (hi - lo).max(0.001);
        let mut seek_to = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (g, tip) = if self.playing {
                (icon::PAUSE, "Pause (Space)")
            } else {
                (icon::PLAY, "Play (Space)")
            };
            if widgets::icon_button(ui, g, tip, tip, false).clicked() {
                self.toggle();
            }
            let shown = self
                .scrub
                .map(|f| f * dur)
                .unwrap_or(self.pos - lo)
                .clamp(0.0, dur);
            ui.label(
                egui::RichText::new(format!("{} / {}", clock(shown), clock(dur)))
                    .size(12.0)
                    .color(p.text_2),
            );
            let bar_w = (ui.available_width() - 44.0).max(60.0);
            if let Some(f) = bar(ui, bar_w, shown / dur, &mut self.scrub) {
                seek_to = Some(lo + f * dur);
            }
            let (g, tip) = if self.muted {
                (icon::MUTE, "Unmute")
            } else {
                (icon::VOLUME, "Mute")
            };
            if widgets::icon_button(ui, g, tip, tip, false).clicked() {
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
