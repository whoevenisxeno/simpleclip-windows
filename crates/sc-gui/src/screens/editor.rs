use crate::app::{ScApp, Screen, ToastKind};
use crate::player::{Player, View};
use crate::theme::{self, icon, pal_ui};
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{pos2, vec2, Color32, Key, Modifiers, Rect, RichText, Rounding, Sense, Stroke, Ui};
use sc_core::edit::{Crop, Plan, AUDIO_SHIFT_MAX, MIN_CROP};
use sc_ffmpeg::EditProgress;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const RULER: f32 = 18.0;
const TRACK: f32 = 58.0;
const STRIP_TILES: usize = 14;
const SEEK_EVERY: Duration = Duration::from_millis(110);
const SPEEDS: [(f64, &str); 7] = [
    (0.25, "0.25\u{d7}"),
    (0.5, "0.5\u{d7}"),
    (1.0, "1\u{d7}"),
    (1.5, "1.5\u{d7}"),
    (2.0, "2\u{d7}"),
    (3.0, "3\u{d7}"),
    (4.0, "4\u{d7}"),
];

#[derive(Clone, Copy, PartialEq, Default, Debug)]
enum SaveMode {
    #[default]
    Copy,
    Replace,
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Tool {
    #[default]
    Speed,
    Sound,
    Fades,
    Crop,
    Rotate,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shape {
    Free,
    Wide,
    Tall,
    Square,
    Portrait,
    Cinema,
}

impl Shape {
    fn ratio(self) -> Option<f64> {
        match self {
            Shape::Free => None,
            Shape::Wide => Some(16.0 / 9.0),
            Shape::Tall => Some(9.0 / 16.0),
            Shape::Square => Some(1.0),
            Shape::Portrait => Some(4.0 / 5.0),
            Shape::Cinema => Some(21.0 / 9.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Grip {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

struct Strip {
    rx: Receiver<(usize, u32, u32, Vec<u8>)>,
    tiles: Vec<Option<egui::TextureHandle>>,
}

#[derive(Debug)]
struct Done {
    path: PathBuf,
    secs: f64,
}

struct Job {
    rx: Receiver<Result<Done, String>>,
    progress: Arc<EditProgress>,
    replace: bool,
    precise: bool,
}

#[derive(Default)]
pub struct EditorState {
    path: PathBuf,
    error: Option<String>,
    duration: f64,
    has_audio: bool,
    plan: Plan,
    undo: Vec<Plan>,
    redo: Vec<Plan>,
    hold: Option<Plan>,
    stepped: bool,
    selected: usize,
    tool: Tool,
    shape: Option<Shape>,
    mode: SaveMode,
    precise: bool,
    save_open: bool,
    result: Option<PathBuf>,
    scrub: Option<f64>,
    last_seek: Option<Instant>,
    saved_volume: f32,
    strip: Option<Strip>,
    job: Option<Job>,
    player: Option<Player>,
}

impl EditorState {
    pub fn stop_player(&mut self) {
        self.player = None;
    }

    pub fn demo(&mut self, name: &str) {
        if self.duration <= 0.0 {
            return;
        }
        let d = self.duration;
        if self.plan.segments.len() == 1 {
            self.plan.split(d * 0.3);
            self.plan.split(d * 0.55);
            self.plan.remove(1);
            self.plan.set_speed(0, 0.5);
            self.plan.set_speed(1, 2.0);
        }
        self.selected = 0;
        self.plan.crop = None;
        self.plan.turn = 0;
        self.plan.flip_h = false;
        self.plan.fade_in = 0.0;
        self.plan.fade_out = 0.0;
        self.plan.audio_shift = 0.0;
        self.tool = Tool::Speed;
        self.save_open = false;
        match name {
            "editor-save" => {
                self.save_open = true;
            }
            "editor-sound" => {
                self.tool = Tool::Sound;
                self.plan.audio_shift = 0.12;
            }
            "editor-fades" => {
                self.tool = Tool::Fades;
                self.plan.fade_in = 2.0;
                self.plan.fade_out = 1.5;
            }
            "editor-crop" => {
                self.tool = Tool::Crop;
                self.shape = Some(Shape::Square);
                let (w, h) = self.player.as_ref().map(Player::size).unwrap_or((16, 9));
                self.plan.crop = Some(Crop::with_ratio(w as f64, h as f64, 1.0));
            }
            "editor-rotate" => {
                self.tool = Tool::Rotate;
                self.plan.turn = 90;
                self.plan.flip_h = true;
            }
            _ => {}
        }
        if let Some(p) = self.player.as_mut() {
            p.seek(d * 0.12);
        }
    }

    fn load(&mut self, path: &Path) {
        *self = Self {
            path: path.to_path_buf(),
            precise: true,
            saved_volume: 1.0,
            ..Default::default()
        };
        let mut player = Player::open_paused(path, 0.0);
        let (info, _) = (sc_ffmpeg::media_info(path), ());
        match info {
            Ok(i) if i.duration > 0.0 => {
                self.duration = i.duration;
                self.has_audio = i.audio_streams > 0;
                self.plan = Plan::whole(i.duration);
                player.set_segments(vec![(0.0, i.duration, 1.0)]);
                self.strip = Some(spawn_strip(path, i.duration));
                self.player = Some(player);
            }
            Ok(_) => self.error = Some("This clip has no length to edit.".into()),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn commit(&mut self, before: &Plan, ctx: &egui::Context) {
        if std::mem::take(&mut self.stepped) {
            self.hold = None;
            return;
        }
        if self.plan != *before && self.hold.is_none() {
            self.hold = Some(before.clone());
        }
        if self.hold.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            if let Some(h) = self.hold.take() {
                if h != self.plan {
                    self.undo.push(h);
                    self.redo.clear();
                    if self.undo.len() > 100 {
                        self.undo.remove(0);
                    }
                }
            }
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.plan, prev));
            self.after_history();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.plan, next));
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.selected = self
            .selected
            .min(self.plan.segments.len().saturating_sub(1));
        self.shape = None;
        self.hold = None;
        self.stepped = true;
    }

    fn reset(&mut self) {
        if self.plan == Plan::whole(self.duration) {
            return;
        }
        self.undo.push(self.plan.clone());
        self.redo.clear();
        self.plan = Plan::whole(self.duration);
        self.selected = 0;
        self.shape = None;
        self.hold = None;
        self.stepped = true;
    }

    fn position(&self) -> f64 {
        self.player.as_ref().map(Player::position).unwrap_or(0.0)
    }

    fn view(&self) -> View {
        View {
            turn: self.plan.turn,
            flip_h: self.plan.flip_h,
            flip_v: self.plan.flip_v,
            dim: 1.0 - self.fade_now(),
        }
    }

    fn fade_now(&self) -> f32 {
        if self.plan.fade_in <= 0.0 && self.plan.fade_out <= 0.0 {
            return 1.0;
        }
        match self.plan.output_time(self.position()) {
            Some(t) => self.plan.fade_gain(t, self.plan.output_secs()),
            None => 1.0,
        }
    }

    fn shown_size(&self) -> (f64, f64) {
        let (w, h) = self.player.as_ref().map(Player::size).unwrap_or((16, 9));
        let (w, h) = self.plan.oriented(w, h);
        (w as f64, h as f64)
    }

    fn do_split(&mut self) -> bool {
        let at = self.position();
        self.do_split_at(at)
    }

    fn do_split_at(&mut self, at: f64) -> bool {
        match self.plan.split(at) {
            Some(i) => {
                self.selected = i;
                true
            }
            None => false,
        }
    }

    fn do_delete(&mut self) -> bool {
        if self.plan.remove(self.selected) {
            self.selected = self.selected.min(self.plan.segments.len() - 1);
            true
        } else {
            false
        }
    }

    fn seek(&mut self, t: f64) {
        if let Some(p) = self.player.as_mut() {
            p.seek(t);
        }
    }

    fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        let pieces = self.plan.segments.len();
        if pieces > 1 {
            out.push(format!("{pieces} pieces"));
        }
        if self.plan.has_speed_change() {
            let changed = self
                .plan
                .segments
                .iter()
                .filter(|s| (s.speed - 1.0).abs() > 1e-6)
                .count();
            out.push(if pieces == 1 {
                format!("{}\u{d7} speed", trim_num(self.plan.segments[0].speed))
            } else {
                format!("speed changed on {changed} of {pieces}")
            });
        }
        if self.has_audio && self.plan.has_audio_shift() {
            let ms = (self.plan.audio_shift * 1000.0).round() as i32;
            out.push(format!(
                "sound {} ms {}",
                ms.abs(),
                if ms > 0 { "later" } else { "earlier" }
            ));
        }
        if self.has_audio {
            if self.plan.volume < 0.001 {
                out.push("muted".into());
            } else if (self.plan.volume - 1.0).abs() > 1e-6 {
                out.push(format!("{:.0}% volume", self.plan.volume * 100.0));
            }
        }
        if self.plan.fade_in > 0.0 {
            out.push(format!("{}s fade in", trim_num(self.plan.fade_in)));
        }
        if self.plan.fade_out > 0.0 {
            out.push(format!("{}s fade out", trim_num(self.plan.fade_out)));
        }
        if self.plan.crop.is_some() {
            out.push("cropped".into());
        }
        if self.plan.turn != 0 {
            out.push(format!("turned {}\u{b0}", self.plan.turn));
        }
        if self.plan.flip_h {
            out.push("flipped sideways".into());
        }
        if self.plan.flip_v {
            out.push("flipped upside down".into());
        }
        out
    }
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0);
    format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
}

fn tick_label(secs: f64, fine: bool) -> String {
    let s = secs.max(0.0);
    if fine {
        format!("{}:{:04.1}", (s / 60.0) as u32, s % 60.0)
    } else {
        format!("{}:{:02}", (s / 60.0) as u32, (s % 60.0) as u32)
    }
}

fn spawn_strip(path: &Path, duration: f64) -> Strip {
    let (tx, rx) = crossbeam_channel::bounded(STRIP_TILES);
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        for i in 0..STRIP_TILES {
            let t = (i as f64 + 0.5) / STRIP_TILES as f64 * duration;
            if let Ok((w, h, rgb)) = sc_ffmpeg::frame_at(&path, t, 220) {
                if tx.send((i, w, h, rgb)).is_err() {
                    return;
                }
            }
        }
    });
    Strip {
        rx,
        tiles: (0..STRIP_TILES).map(|_| None).collect(),
    }
}

fn poll_strip(st: &mut EditorState, ctx: &egui::Context) {
    let Some(strip) = st.strip.as_mut() else {
        return;
    };
    loop {
        match strip.rx.try_recv() {
            Ok((i, w, h, rgb)) => {
                let img = egui::ColorImage::from_rgb([w as usize, h as usize], &rgb);
                if let Some(slot) = strip.tiles.get_mut(i) {
                    *slot = Some(ctx.load_texture(
                        format!("strip-{i}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(120));
                break;
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
}

pub fn view(app: &mut ScApp, ui: &mut Ui, path: &Path) {
    let p = pal_ui(ui);
    if app.editor.path != path {
        app.editor.load(path);
    }
    let ctx = ui.ctx().clone();
    poll_job(app, &ctx);
    poll_strip(&mut app.editor, &ctx);

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("clip")
        .to_string();
    let busy = app.editor.job.is_some();
    let mut leave = false;
    let (mut undo, mut redo, mut reset, mut save) = (false, false, false, false);
    ui.horizontal(|ui| {
        if widgets::quiet_button(ui, Some(icon::BACK), "Clips").clicked() {
            leave = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let st = &app.editor;
            ui.add_enabled_ui(!busy && st.error.is_none(), |ui| {
                save = widgets::primary_button(ui, Some(icon::SAVE), "Save\u{2026}").clicked();
                ui.add_space(6.0);
                reset = widgets::quiet_button(ui, None, "Reset").clicked();
                redo = ui
                    .add_enabled_ui(!st.redo.is_empty(), |ui| {
                        widgets::quiet_button(ui, None, "Redo").clicked()
                    })
                    .inner;
                undo = ui
                    .add_enabled_ui(!st.undo.is_empty(), |ui| {
                        widgets::quiet_button(ui, None, "Undo").clicked()
                    })
                    .inner;
            });
        });
    });
    if leave {
        app.goto(Screen::Gallery);
        return;
    }
    widgets::page_title(ui, "Edit", Some(&name));

    if let Some(err) = app.editor.error.clone() {
        widgets::notice(ui, p.red, &err);
        ui.add_space(8.0);
        if widgets::quiet_button(ui, Some(icon::REFRESH), "Try again").clicked() {
            app.editor.path = PathBuf::new();
        }
        return;
    }
    if app.editor.player.is_none() {
        widgets::secondary(ui, "Reading clip\u{2026}");
        return;
    }

    if save {
        app.editor.save_open = true;
    }
    let before = app.editor.plan.clone();
    if !busy && !app.editor.save_open {
        if undo {
            app.editor.undo();
        }
        if redo {
            app.editor.redo();
        }
        if reset {
            app.editor.reset();
        }
        keys(app, &ctx);
    }

    let (segs, gain) = {
        let st = &app.editor;
        (
            st.plan
                .segments
                .iter()
                .map(|s| (s.start, s.end, s.speed))
                .collect(),
            st.plan.volume * st.fade_now(),
        )
    };
    let shift = app.editor.plan.audio_shift;
    let view_state = app.editor.view();
    let crop_tool = app.editor.tool == Tool::Crop;
    let mut toggle_play = false;
    if let Some(pl) = app.editor.player.as_mut() {
        pl.set_segments(segs);
        pl.set_gain(gain);
        pl.set_audio_shift(shift);
    }
    let frame = {
        let st = &mut app.editor;
        st.player.as_mut().and_then(|pl| pl.frame(ui, view_state))
    };
    if let Some((rect, resp)) = frame {
        let st = &mut app.editor;
        if crop_tool {
            crop_overlay(ui, st, rect);
        } else if resp.clicked() {
            toggle_play = true;
        }
        let rate = st.player.as_ref().map(Player::rate).unwrap_or(1.0);
        if (rate - 1.0).abs() > 1e-6 {
            speed_badge(ui, rect, rate);
        }
    }
    if toggle_play {
        if let Some(pl) = app.editor.player.as_mut() {
            pl.toggle();
        }
    }
    ui.add_space(10.0);

    if let Some(message) = transport(&mut app.editor, ui) {
        app.toast(message, ToastKind::Warn);
    }
    ui.add_space(8.0);
    tools(&mut app.editor, ui);
    if result_notice(&mut app.editor, ui) {
        app.gallery.show_trimmed();
        app.goto(Screen::Gallery);
        return;
    }
    let trim_dir = app.cfg.save.trim_directory.clone();
    if save_dialog(&mut app.editor, &trim_dir, &ctx) == SaveAction::Start {
        start_export(app, trim_dir);
    }

    let st = &mut app.editor;
    st.commit(&before, &ctx);
    if st.plan != before {
        ctx.request_repaint();
    }
}

fn keys(app: &mut ScApp, ctx: &egui::Context) {
    if let Some(message) = handle_keys(&mut app.editor, ctx) {
        app.toast(message, ToastKind::Warn);
    }
}

fn handle_keys(st: &mut EditorState, ctx: &egui::Context) -> Option<&'static str> {
    if ctx.wants_keyboard_input() {
        return None;
    }
    let (undo, redo, save, space, split, delete, left, right, shift) = ctx.input_mut(|i| {
        let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
            || i.consume_key(Modifiers::COMMAND, Key::Y);
        let undo = i.consume_key(Modifiers::COMMAND, Key::Z);
        let save = i.consume_key(Modifiers::COMMAND, Key::S);
        (
            undo,
            redo,
            save,
            i.key_pressed(Key::Space),
            i.key_pressed(Key::S) && !i.modifiers.any(),
            i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            i.key_pressed(Key::ArrowLeft),
            i.key_pressed(Key::ArrowRight),
            i.modifiers.shift,
        )
    });
    if undo {
        st.undo();
    }
    if redo {
        st.redo();
    }
    if save {
        st.save_open = true;
    }
    if space {
        if let Some(p) = st.player.as_mut() {
            p.toggle();
        }
    }
    if split && !st.do_split() {
        return Some("Move the playhead inside a piece to split it");
    }
    if delete && !st.do_delete() {
        return Some("A clip needs at least one piece");
    }
    let step = if shift { 5.0 } else { 1.0 };
    if left || right {
        let to = (st.position() + if left { -step } else { step }).clamp(0.0, st.duration);
        st.seek(to);
    }
    None
}

fn speed_badge(ui: &Ui, rect: Rect, speed: f64) {
    let p = pal_ui(ui);
    let text = format!("{}\u{d7}", trim_num(speed));
    let font = egui::FontId::new(12.0, theme::semibold());
    let galley = ui.painter().layout_no_wrap(text, font, Color32::WHITE);
    let pill = Rect::from_min_size(
        pos2(rect.right() - galley.size().x - 26.0, rect.top() + 10.0),
        galley.size() + vec2(16.0, 8.0),
    );
    ui.painter().rect_filled(
        pill,
        Rounding::same(pill.height() / 2.0),
        Color32::from_black_alpha(150),
    );
    ui.painter()
        .galley(pill.center() - galley.size() / 2.0, galley, p.text);
}

fn transport(st: &mut EditorState, ui: &mut Ui) -> Option<&'static str> {
    let p = pal_ui(ui);
    let mut play = false;
    let mut split = false;
    let mut delete = false;
    let pos = st.scrub.unwrap_or_else(|| st.position());
    let playing = st.player.as_ref().is_some_and(Player::is_playing);
    let can_delete = st.plan.segments.len() > 1;
    let busy = st.job.is_some();
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (g, tip) = if playing {
                (icon::PAUSE, "Pause (Space)")
            } else {
                (icon::PLAY, "Play (Space)")
            };
            play = widgets::icon_button(ui, g, tip, tip, false).clicked();
            ui.label(
                RichText::new(format!("{} / {}", clock(pos), clock(st.duration)))
                    .size(13.0)
                    .color(p.text_2),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.add_enabled_ui(can_delete, |ui| {
                        delete =
                            widgets::quiet_button(ui, Some(icon::DELETE), "Delete piece").clicked();
                    });
                    split = widgets::quiet_button(ui, Some(icon::CUT), "Split here").clicked();
                });
            });
        });
        ui.add_space(6.0);
        if let Some(t) = st.timeline(ui, pos) {
            st.seek(t);
        }
        ui.add_space(2.0);
        let kept = st.plan.kept_secs();
        let out = st.plan.output_secs();
        let pieces = st.plan.segments.len();
        let sel = st.selected.min(pieces.saturating_sub(1));
        let mut parts: Vec<String> = Vec::new();
        if pieces > 1 {
            let seg = st.plan.segments[sel];
            let mut line = format!(
                "Piece {} of {}: {} to {}",
                sel + 1,
                pieces,
                clock(seg.start),
                clock(seg.end)
            );
            if (seg.speed - 1.0).abs() > 1e-6 {
                line.push_str(&format!(" at {}\u{d7}", trim_num(seg.speed)));
            }
            parts.push(line);
        }
        if (kept - st.duration).abs() >= 0.05 || (out - kept).abs() >= 0.05 {
            parts.push(format!(
                "Result {} from {} kept of {}",
                clock(out),
                clock(kept),
                clock(st.duration)
            ));
        }
        let msg = if parts.is_empty() {
            "Click to move the playhead. S splits, Delete removes the selected piece.".to_string()
        } else {
            format!("{}.", parts.join(". "))
        };
        ui.label(RichText::new(msg).size(12.0).color(p.text_3));
    });
    if play {
        if let Some(pl) = st.player.as_mut() {
            pl.toggle();
        }
    }
    if split && !st.do_split() {
        Some("Move the playhead inside a piece to split it")
    } else if delete && !st.do_delete() {
        Some("A clip needs at least one piece")
    } else {
        None
    }
}

impl EditorState {
    fn timeline(&mut self, ui: &mut Ui, pos: f64) -> Option<f64> {
        let p = pal_ui(ui);
        let dur = self.duration.max(0.1);
        let w = ui.available_width();
        let (outer, _) = ui.allocate_exact_size(vec2(w, RULER + TRACK + 6.0), Sense::hover());
        let track = Rect::from_min_max(
            pos2(outer.left() + 9.0, outer.top() + RULER),
            pos2(outer.right() - 9.0, outer.top() + RULER + TRACK),
        );
        let x_of = |t: f64| track.left() + (t / dur) as f32 * track.width();
        let t_of =
            |x: f32| (((x - track.left()) / track.width().max(1.0)) as f64 * dur).clamp(0.0, dur);
        let painter = ui.painter().clone();

        let steps = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0];
        let step = steps
            .iter()
            .copied()
            .find(|s| s / dur * track.width() as f64 >= 64.0)
            .unwrap_or(300.0);
        let mut k = 0.0;
        while k <= dur + 1e-6 {
            let x = x_of(k);
            painter.line_segment(
                [pos2(x, track.top() - 6.0), pos2(x, track.top() - 2.0)],
                Stroke::new(1.0_f32, p.text_3),
            );
            painter.text(
                pos2(x + 3.0, track.top() - 8.0),
                egui::Align2::LEFT_BOTTOM,
                tick_label(k, step < 1.0),
                egui::FontId::new(10.5, egui::FontFamily::Proportional),
                p.text_3,
            );
            k += step;
        }

        let round = Rounding::same(6.0);
        painter.rect_filled(track, round, p.card_hi);
        if let Some(strip) = &self.strip {
            let n = strip.tiles.len().max(1);
            let tile_w = track.width() / n as f32;
            let clipped = painter.with_clip_rect(track);
            for (i, tile) in strip.tiles.iter().enumerate() {
                let Some(tex) = tile else { continue };
                let r = Rect::from_min_size(
                    pos2(track.left() + tile_w * i as f32, track.top()),
                    vec2(tile_w + 0.5, TRACK),
                );
                let [tw, th] = tex.size();
                let ta = tw as f32 / th.max(1) as f32;
                let ra = r.width() / r.height();
                let uv = if ra < ta {
                    let keep = ra / ta;
                    Rect::from_min_max(pos2((1.0 - keep) / 2.0, 0.0), pos2((1.0 + keep) / 2.0, 1.0))
                } else {
                    let keep = ta / ra;
                    Rect::from_min_max(pos2(0.0, (1.0 - keep) / 2.0), pos2(1.0, (1.0 + keep) / 2.0))
                };
                clipped.image(tex.id(), r, uv, Color32::WHITE);
            }
        }

        let mut gaps: Vec<(f64, f64)> = Vec::new();
        let mut cursor = 0.0;
        for s in &self.plan.segments {
            if s.start > cursor + 1e-6 {
                gaps.push((cursor, s.start));
            }
            cursor = s.end;
        }
        if cursor < dur - 1e-6 {
            gaps.push((cursor, dur));
        }
        for (a, b) in gaps {
            let r = Rect::from_min_max(pos2(x_of(a), track.top()), pos2(x_of(b), track.bottom()));
            let g = painter.with_clip_rect(r);
            g.rect_filled(r, 0.0, Color32::from_black_alpha(205));
            let mut x = r.left() - TRACK;
            while x < r.right() {
                g.line_segment(
                    [pos2(x, track.bottom()), pos2(x + TRACK, track.top())],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(28)),
                );
                x += 9.0;
            }
        }
        self.selected = self
            .selected
            .min(self.plan.segments.len().saturating_sub(1));

        let base = ui.interact(
            Rect::from_min_max(outer.min, outer.max),
            ui.id().with("timeline"),
            Sense::click_and_drag(),
        );
        let mut seek = None;
        let throttled = self.last_seek.is_none_or(|t| t.elapsed() >= SEEK_EVERY);
        if base.clicked() || base.drag_started() || base.dragged() {
            if let Some(pt) = base.interact_pointer_pos() {
                let t = t_of(pt.x);
                if base.clicked() || base.drag_started() {
                    if let Some(i) = self.plan.segment_at(t) {
                        self.selected = i;
                    }
                }
                if base.clicked() {
                    self.scrub = None;
                    seek = Some(t);
                } else {
                    self.scrub = Some(t);
                    if throttled {
                        seek = Some(t);
                    }
                }
            }
        }
        if base.drag_stopped() {
            if let Some(t) = self.scrub.take() {
                seek = Some(t);
            }
        }

        let sel = self.selected;
        for is_start in [true, false] {
            let Some(seg) = self.plan.segments.get(sel).copied() else {
                break;
            };
            let edge = if is_start { seg.start } else { seg.end };
            let x = x_of(edge);
            let hit = Rect::from_center_size(pos2(x, track.center().y), vec2(18.0, TRACK + 8.0));
            let resp = ui.interact(hit, ui.id().with(("edge", sel, is_start)), Sense::drag());
            if resp.dragged() {
                if let Some(pt) = resp.interact_pointer_pos() {
                    self.plan
                        .move_edge(sel, is_start, t_of(pt.x), self.duration);
                    let e = if is_start {
                        self.plan.segments[sel].start
                    } else {
                        self.plan.segments[sel].end
                    };
                    self.scrub = Some(e);
                    if throttled {
                        seek = Some(e);
                    }
                }
            }
            if resp.drag_stopped() {
                if let Some(t) = self.scrub.take() {
                    seek = Some(t);
                }
            }
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if seek.is_some() {
            self.last_seek = Some(Instant::now());
        }

        for (i, seg) in self.plan.segments.iter().enumerate() {
            let r = Rect::from_min_max(
                pos2(x_of(seg.start), track.top()),
                pos2(x_of(seg.end), track.bottom()),
            );
            let stroke = if i == sel {
                Stroke::new(2.5_f32, p.accent)
            } else {
                Stroke::new(1.5_f32, Color32::from_white_alpha(120))
            };
            painter.rect_stroke(r.shrink(1.0), Rounding::same(5.0), stroke);
            if (seg.speed - 1.0).abs() > 1e-6 && r.width() > 46.0 {
                let galley = painter.layout_no_wrap(
                    format!("{}\u{d7}", trim_num(seg.speed)),
                    egui::FontId::new(11.0, theme::semibold()),
                    Color32::WHITE,
                );
                let pill = Rect::from_min_size(
                    r.left_top() + vec2(8.0, 6.0),
                    galley.size() + vec2(10.0, 4.0),
                );
                painter.rect_filled(
                    pill,
                    Rounding::same(pill.height() / 2.0),
                    Color32::from_black_alpha(170),
                );
                painter.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
            }
        }
        if let Some(seg) = self.plan.segments.get(sel) {
            for is_start in [true, false] {
                let x = x_of(if is_start { seg.start } else { seg.end });
                let bar =
                    Rect::from_center_size(pos2(x, track.center().y), vec2(10.0, TRACK + 4.0));
                painter.rect_filled(
                    bar.translate(vec2(0.0, 1.0)),
                    Rounding::same(4.0),
                    Color32::from_black_alpha(60),
                );
                painter.rect_filled(bar, Rounding::same(4.0), Color32::WHITE);
                painter.rect_filled(
                    Rect::from_center_size(bar.center(), vec2(2.0, 18.0)),
                    Rounding::same(1.0),
                    Color32::from_rgb(0xc7, 0xc7, 0xcc),
                );
            }
        }

        let head = x_of(self.scrub.unwrap_or(pos).clamp(0.0, dur));
        painter.line_segment(
            [
                pos2(head, track.top() - 4.0),
                pos2(head, track.bottom() + 3.0),
            ],
            Stroke::new(2.0_f32, Color32::from_black_alpha(90)),
        );
        painter.line_segment(
            [
                pos2(head, track.top() - 4.0),
                pos2(head, track.bottom() + 3.0),
            ],
            Stroke::new(1.5_f32, Color32::WHITE),
        );
        painter.circle_filled(pos2(head, track.top() - 5.0), 5.0, Color32::WHITE);
        painter.circle_stroke(
            pos2(head, track.top() - 5.0),
            5.0,
            Stroke::new(1.5_f32, p.accent),
        );
        seek
    }
}

fn tools(st: &mut EditorState, ui: &mut Ui) {
    let busy = st.job.is_some();
    ui.horizontal(|ui| {
        widgets::segmented(
            ui,
            &mut st.tool,
            &[
                (Tool::Speed, "Speed"),
                (Tool::Sound, "Sound"),
                (Tool::Fades, "Fades"),
                (Tool::Crop, "Crop"),
                (Tool::Rotate, "Rotate"),
            ],
        );
    });
    ui.add_space(10.0);
    ui.add_enabled_ui(!busy, |ui| match st.tool {
        Tool::Speed => speed_tool(st, ui),
        Tool::Sound => sound_tool(st, ui),
        Tool::Fades => fade_tool(st, ui),
        Tool::Crop => crop_tool(st, ui),
        Tool::Rotate => rotate_tool(st, ui),
    });
}

fn speed_tool(st: &mut EditorState, ui: &mut Ui) {
    let pieces = st.plan.segments.len();
    let sel = st.selected.min(pieces.saturating_sub(1));
    let current = st.plan.segments[sel].speed;
    let uneven = st
        .plan
        .segments
        .iter()
        .any(|s| (s.speed - current).abs() > 1e-9);
    let title = if pieces > 1 {
        format!("Speed of piece {} of {}", sel + 1, pieces)
    } else {
        "Speed".to_string()
    };
    let hint = if pieces > 1 {
        "Pick a piece on the timeline, then its speed. Sound keeps its pitch."
    } else {
        "Split the clip to slow down or speed up just one part. Sound keeps its pitch."
    };
    let mut apply_all = false;
    widgets::section(ui, None, |s| {
        s.row(&title, Some(hint), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let mut value = current;
            if widgets::segmented(ui, &mut value, &SPEEDS) {
                st.plan.set_speed(sel, value);
            }
        });
        if pieces > 1 && uneven {
            s.row(
                "All pieces",
                Some("Give every piece the speed of the selected one."),
                |ui| {
                    apply_all = widgets::quiet_button(ui, None, "Apply to all").clicked();
                },
            );
        }
    });
    if apply_all {
        st.plan.set_all_speeds(current);
    }
}

fn sound_tool(st: &mut EditorState, ui: &mut Ui) {
    if !st.has_audio {
        widgets::section(ui, None, |s| {
            s.row(
                "No sound",
                Some("This clip has no audio to change."),
                |_| {},
            );
        });
        return;
    }
    let mut muted = st.plan.volume < 0.001;
    let was_muted = muted;
    widgets::section(ui, None, |s| {
        s.row(
            "Volume",
            Some("100% keeps the sound as recorded. Double click the slider to reset it."),
            |ui| {
                let mut pct = (st.plan.volume * 100.0).round() as u32;
                widgets::value_label(
                    ui,
                    &if pct == 0 {
                        "Muted".to_string()
                    } else {
                        format!("{pct}%")
                    },
                );
                if widgets::percent_slider(ui, &mut pct, 200).changed() {
                    st.plan.volume = pct as f32 / 100.0;
                    if pct > 0 {
                        st.saved_volume = st.plan.volume;
                    }
                }
            },
        );
        s.row(
            "Mute",
            Some("Removes all sound from the saved clip."),
            |ui| {
                widgets::toggle(ui, &mut muted);
            },
        );
        s.row(
            "Sound timing",
            Some("Slide or nudge the sound until it matches the picture."),
            |ui| {
                let ms = (st.plan.audio_shift * 1000.0).round() as i32;
                widgets::value_label(
                    ui,
                    &if ms == 0 {
                        "In sync".to_string()
                    } else {
                        format!("{ms:+} ms")
                    },
                );
                let mut v = (ms / 10 + 300).clamp(0, 600) as u32;
                if widgets::step_slider(ui, &mut v, 600, 300).changed() {
                    st.plan.audio_shift = (v as f64 - 300.0) / 100.0;
                }
                let nudge = |shift: f64, by: f64| {
                    ((shift + by) * 100.0)
                        .round()
                        .clamp(-AUDIO_SHIFT_MAX * 100.0, AUDIO_SHIFT_MAX * 100.0)
                        / 100.0
                };
                if widgets::quiet_button(ui, None, "Later").clicked() {
                    st.plan.audio_shift = nudge(st.plan.audio_shift, 0.01);
                }
                if widgets::quiet_button(ui, None, "Earlier").clicked() {
                    st.plan.audio_shift = nudge(st.plan.audio_shift, -0.01);
                }
            },
        );
    });
    if muted != was_muted {
        if muted {
            st.saved_volume = if st.plan.volume > 0.001 {
                st.plan.volume
            } else {
                1.0
            };
            st.plan.volume = 0.0;
        } else {
            st.plan.volume = st.saved_volume.max(0.05);
        }
    }
}

fn fade_tool(st: &mut EditorState, ui: &mut Ui) {
    let label = |secs: f64| {
        if secs <= 0.0 {
            "Off".to_string()
        } else {
            format!("{} s", trim_num(secs))
        }
    };
    widgets::section(ui, None, |s| {
        s.row(
            "Fade in",
            Some("Starts from black and silence. Double click the slider to turn it off."),
            |ui| {
                let mut tenths = (st.plan.fade_in * 10.0).round() as u32;
                widgets::value_label(ui, &label(st.plan.fade_in));
                if widgets::step_slider(ui, &mut tenths, 50, 0).changed() {
                    st.plan.fade_in = tenths as f64 / 10.0;
                }
            },
        );
        s.row("Fade out", Some("Ends in black and silence."), |ui| {
            let mut tenths = (st.plan.fade_out * 10.0).round() as u32;
            widgets::value_label(ui, &label(st.plan.fade_out));
            if widgets::step_slider(ui, &mut tenths, 50, 0).changed() {
                st.plan.fade_out = tenths as f64 / 10.0;
            }
        });
    });
}

fn rotate_tool(st: &mut EditorState, ui: &mut Ui) {
    let before = (st.plan.turn, st.plan.flip_h, st.plan.flip_v);
    widgets::section(ui, None, |s| {
        s.row(
            "Rotate",
            Some("Turns the picture a quarter at a time."),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if widgets::quiet_button(ui, None, "Turn right").clicked() {
                    st.plan.turn = (st.plan.turn + 90) % 360;
                }
                if widgets::quiet_button(ui, None, "Turn left").clicked() {
                    st.plan.turn = (st.plan.turn + 270) % 360;
                }
            },
        );
        s.row("Flip", Some("Mirrors the picture."), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let v = if st.plan.flip_v {
                "Upside down: on"
            } else {
                "Upside down"
            };
            if widgets::quiet_button(ui, None, v).clicked() {
                st.plan.flip_v = !st.plan.flip_v;
            }
            let h = if st.plan.flip_h {
                "Sideways: on"
            } else {
                "Sideways"
            };
            if widgets::quiet_button(ui, None, h).clicked() {
                st.plan.flip_h = !st.plan.flip_h;
            }
        });
    });
    if before != (st.plan.turn, st.plan.flip_h, st.plan.flip_v) {
        st.plan.crop = None;
        st.shape = None;
    }
}

fn crop_tool(st: &mut EditorState, ui: &mut Ui) {
    let mut pick = st.shape;
    let mut picked = false;
    let mut clear = false;
    widgets::section(ui, None, |s| {
        s.row(
            "Shape",
            Some("Drag the corners above. A shape locks the proportions."),
            |ui| {
                if st.plan.crop.is_some() {
                    clear = widgets::quiet_button(ui, None, "Clear").clicked();
                }
                picked = widgets::segmented(
                    ui,
                    &mut pick,
                    &[
                        (Some(Shape::Free), "Free"),
                        (Some(Shape::Wide), "16:9"),
                        (Some(Shape::Tall), "9:16"),
                        (Some(Shape::Square), "1:1"),
                        (Some(Shape::Portrait), "4:5"),
                        (Some(Shape::Cinema), "21:9"),
                    ],
                );
            },
        );
    });
    if clear {
        st.plan.crop = None;
        st.shape = None;
        return;
    }
    if let (true, Some(pick)) = (picked, pick) {
        st.shape = Some(pick);
        let (w, h) = st.shown_size();
        st.plan.crop = Some(match pick.ratio() {
            Some(r) => Crop::with_ratio(w, h, r).clamped(),
            None => st.plan.crop.unwrap_or(Crop {
                x: 0.1,
                y: 0.1,
                w: 0.8,
                h: 0.8,
            }),
        });
    }
}

fn crop_overlay(ui: &mut Ui, st: &mut EditorState, rect: Rect) {
    let Some(crop) = st.plan.crop else {
        return;
    };
    let p = pal_ui(ui);
    let to_screen = |c: Crop| {
        Rect::from_min_size(
            pos2(
                rect.left() + c.x as f32 * rect.width(),
                rect.top() + c.y as f32 * rect.height(),
            ),
            vec2(c.w as f32 * rect.width(), c.h as f32 * rect.height()),
        )
    };
    let area = to_screen(crop);
    let dim = Color32::from_black_alpha(150);
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_max(rect.min, pos2(rect.right(), area.top())),
        0.0,
        dim,
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), area.bottom()), rect.max),
        0.0,
        dim,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.left(), area.top()),
            pos2(area.left(), area.bottom()),
        ),
        0.0,
        dim,
    );
    painter.rect_filled(
        Rect::from_min_max(
            pos2(area.right(), area.top()),
            pos2(rect.right(), area.bottom()),
        ),
        0.0,
        dim,
    );
    painter.rect_stroke(area, 0.0, Stroke::new(1.5_f32, Color32::WHITE));
    for f in [1.0 / 3.0, 2.0 / 3.0] {
        let x = area.left() + area.width() * f;
        let y = area.top() + area.height() * f;
        let line = Stroke::new(1.0_f32, Color32::from_white_alpha(70));
        painter.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], line);
        painter.line_segment([pos2(area.left(), y), pos2(area.right(), y)], line);
    }

    let (fw, fh) = st.shown_size();
    let ratio = st.shape.and_then(Shape::ratio);
    let locked = ratio.map(|r| r * fh / fw);

    let body = ui.interact(area.shrink(10.0), ui.id().with("crop-body"), Sense::drag());
    if body.dragged() {
        let d = body.drag_delta();
        let mut c = crop;
        c.x = (c.x + d.x as f64 / rect.width() as f64).clamp(0.0, 1.0 - c.w);
        c.y = (c.y + d.y as f64 / rect.height() as f64).clamp(0.0, 1.0 - c.h);
        st.plan.crop = Some(c);
    }
    if body.hovered() || body.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let grips: &[Grip] = if locked.is_some() {
        &[
            Grip::TopLeft,
            Grip::TopRight,
            Grip::BottomLeft,
            Grip::BottomRight,
        ]
    } else {
        &[
            Grip::TopLeft,
            Grip::TopRight,
            Grip::BottomLeft,
            Grip::BottomRight,
            Grip::Top,
            Grip::Bottom,
            Grip::Left,
            Grip::Right,
        ]
    };
    for &grip in grips {
        let at = match grip {
            Grip::TopLeft => area.left_top(),
            Grip::TopRight => area.right_top(),
            Grip::BottomLeft => area.left_bottom(),
            Grip::BottomRight => area.right_bottom(),
            Grip::Top => area.center_top(),
            Grip::Bottom => area.center_bottom(),
            Grip::Left => area.left_center(),
            Grip::Right => area.right_center(),
        };
        let hit = Rect::from_center_size(at, vec2(22.0, 22.0));
        let resp = ui.interact(hit, ui.id().with(("crop-grip", grip as u8)), Sense::drag());
        if resp.dragged() {
            if let Some(pt) = resp.interact_pointer_pos() {
                let nx = ((pt.x - rect.left()) / rect.width()) as f64;
                let ny = ((pt.y - rect.top()) / rect.height()) as f64;
                st.plan.crop = Some(drag_grip(
                    crop,
                    grip,
                    nx.clamp(0.0, 1.0),
                    ny.clamp(0.0, 1.0),
                    locked,
                ));
            }
        }
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(match grip {
                Grip::TopLeft | Grip::BottomRight => egui::CursorIcon::ResizeNwSe,
                Grip::TopRight | Grip::BottomLeft => egui::CursorIcon::ResizeNeSw,
                Grip::Top | Grip::Bottom => egui::CursorIcon::ResizeVertical,
                Grip::Left | Grip::Right => egui::CursorIcon::ResizeHorizontal,
            });
        }
        let corner = matches!(
            grip,
            Grip::TopLeft | Grip::TopRight | Grip::BottomLeft | Grip::BottomRight
        );
        let size = if corner {
            vec2(12.0, 12.0)
        } else {
            vec2(18.0, 6.0)
        };
        let size = if matches!(grip, Grip::Left | Grip::Right) {
            vec2(6.0, 18.0)
        } else {
            size
        };
        let knob = Rect::from_center_size(at, size);
        ui.painter()
            .rect_filled(knob, Rounding::same(3.0), Color32::WHITE);
        ui.painter()
            .rect_stroke(knob, Rounding::same(3.0), Stroke::new(1.0_f32, p.accent));
    }
}

fn drag_grip(c: Crop, grip: Grip, px: f64, py: f64, locked: Option<f64>) -> Crop {
    let (left, top, right, bottom) = (c.x, c.y, c.x + c.w, c.y + c.h);
    let gap = MIN_CROP;
    if let Some(rn) = locked {
        let (ax, ay, sx, sy) = match grip {
            Grip::TopLeft => (right, bottom, -1.0, -1.0),
            Grip::TopRight => (left, bottom, 1.0, -1.0),
            Grip::BottomLeft => (right, top, -1.0, 1.0),
            _ => (left, top, 1.0, 1.0),
        };
        let room_w = if sx > 0.0 { 1.0 - ax } else { ax };
        let room_h = if sy > 0.0 { 1.0 - ay } else { ay };
        let want = (px - ax).abs();
        let w = want.min(room_w).min(room_h * rn).max(gap);
        let h = w / rn;
        return Crop {
            x: if sx > 0.0 { ax } else { ax - w },
            y: if sy > 0.0 { ay } else { ay - h },
            w,
            h,
        }
        .clamped();
    }
    let (mut l, mut t, mut r, mut b) = (left, top, right, bottom);
    match grip {
        Grip::TopLeft => {
            l = px.min(r - gap);
            t = py.min(b - gap);
        }
        Grip::TopRight => {
            r = px.max(l + gap);
            t = py.min(b - gap);
        }
        Grip::BottomLeft => {
            l = px.min(r - gap);
            b = py.max(t + gap);
        }
        Grip::BottomRight => {
            r = px.max(l + gap);
            b = py.max(t + gap);
        }
        Grip::Top => t = py.min(b - gap),
        Grip::Bottom => b = py.max(t + gap),
        Grip::Left => l = px.min(r - gap),
        Grip::Right => r = px.max(l + gap),
    }
    Crop {
        x: l,
        y: t,
        w: r - l,
        h: b - t,
    }
    .clamped()
}

fn result_notice(st: &mut EditorState, ui: &mut Ui) -> bool {
    let Some(path) = st.result.clone() else {
        return false;
    };
    let p = pal_ui(ui);
    let mut see = false;
    ui.add_space(10.0);
    widgets::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Saved {}",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("copy")
                ))
                .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                see = widgets::quiet_button(ui, None, "See edited clips").clicked();
            });
        });
    });
    see
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum SaveAction {
    Idle,
    Start,
}

fn save_dialog(
    st: &mut EditorState,
    trim_dir: &Option<PathBuf>,
    ctx: &egui::Context,
) -> SaveAction {
    if !st.save_open {
        return SaveAction::Idle;
    }
    let p = crate::theme::pal(ctx);
    let (mut start, mut cancel_job, mut close) = (false, false, false);
    let plain = st.plan.is_plain_cut();
    let notes = st.summary();
    let running = st.job.as_ref().map(|j| j.progress.fraction());
    let (sw, sh) = st.player.as_ref().map(Player::size).unwrap_or((16, 9));
    let (ow, oh) = st.plan.output_size(sw, sh);
    let length = clock(st.plan.output_secs());
    let closed = crate::screens::sheet::modal(ctx, "edit-save", 520.0, |ui| {
        ui.label(
            RichText::new("Save your edit")
                .font(egui::FontId::new(20.0, theme::semibold()))
                .color(p.text),
        );
        ui.add_space(2.0);
        let line = if notes.is_empty() {
            format!("{length} \u{b7} {ow} \u{d7} {oh} \u{b7} no changes yet")
        } else {
            format!(
                "{length} \u{b7} {ow} \u{d7} {oh} \u{b7} {}",
                notes.join(", ")
            )
        };
        widgets::secondary(ui, line);
        ui.add_space(14.0);
        if let Some(f) = running {
            let precise = st.job.as_ref().is_some_and(|j| j.precise);
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 70.0).max(80.0);
                let (bar, _) = ui.allocate_exact_size(vec2(w, 8.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar, Rounding::same(4.0), p.card_hi);
                let fill =
                    Rect::from_min_size(bar.min, vec2(bar.width() * f.max(0.02), bar.height()));
                ui.painter()
                    .rect_filled(fill, Rounding::same(4.0), p.accent);
                ui.label(
                    RichText::new(format!("{:.0}%", f * 100.0))
                        .size(13.0)
                        .color(p.text_2),
                );
            });
            ui.add_space(6.0);
            widgets::secondary(
                ui,
                if precise {
                    "Rendering your edit. Nothing in your library changes until it is done."
                } else {
                    "Cutting the clip."
                },
            );
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel_job = widgets::quiet_button(ui, None, "Cancel").clicked();
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        if plain {
            ui.horizontal(|ui| {
                widgets::toggle(ui, &mut st.precise);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new("Frame accurate").size(14.0).color(p.text));
                    ui.label(
                        RichText::new(if st.precise {
                            "Re-encodes so the cut lands exactly where you put it."
                        } else {
                            "An instant lossless cut. It can start up to about 2 s early."
                        })
                        .size(12.0)
                        .color(p.text_2),
                    );
                });
            });
            ui.add_space(12.0);
        }
        widgets::segmented(
            ui,
            &mut st.mode,
            &[
                (SaveMode::Copy, "Save as a copy"),
                (SaveMode::Replace, "Replace the original"),
            ],
        );
        ui.add_space(4.0);
        widgets::secondary(
            ui,
            match st.mode {
                SaveMode::Copy => "Keeps the original and adds the edited clip.",
                SaveMode::Replace => {
                    "Keeps only the edited clip. The original goes to the Recycle Bin, so you can still get it back."
                }
            },
        );
        ui.add_space(4.0);
        widgets::secondary(
            ui,
            match (trim_dir, st.mode) {
                (Some(d), SaveMode::Copy) => format!("Saved to {}.", d.display()),
                _ => "Saved next to the original.".to_string(),
            },
        );
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = match st.mode {
                SaveMode::Copy => "Save edited clip",
                SaveMode::Replace => "Replace original",
            };
            start = widgets::primary_button(ui, Some(icon::CUT), label).clicked();
            close = widgets::quiet_button(ui, None, "Cancel").clicked();
        });
    });
    if running.is_some() {
        if cancel_job {
            if let Some(j) = &st.job {
                j.progress.cancel();
            }
        }
        return SaveAction::Idle;
    }
    if closed || close {
        st.save_open = false;
    }
    if start {
        SaveAction::Start
    } else {
        SaveAction::Idle
    }
}

fn unique_output(dir: &Path, stem: &str) -> PathBuf {
    let mut out = dir.join(format!("{stem}-trim.mp4"));
    let mut n = 2;
    while out.exists() {
        out = dir.join(format!("{stem}-trim{n}.mp4"));
        n += 1;
    }
    out
}

fn output_path(src: &Path, trim_dir: &Option<PathBuf>, replace: bool) -> Result<PathBuf, String> {
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "That file name can’t be edited".to_string())?;
    let beside = src.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = match (trim_dir, replace) {
        (Some(d), false) => {
            std::fs::create_dir_all(d)
                .map_err(|e| format!("Can’t create the folder {}: {e}", d.display()))?;
            d.clone()
        }
        _ => beside,
    };
    Ok(unique_output(&dir, stem))
}

fn start_export(app: &mut ScApp, trim_dir: Option<PathBuf>) {
    let replace = app.editor.mode == SaveMode::Replace;
    let src = app.editor.path.clone();
    let dst = match output_path(&src, &trim_dir, replace) {
        Ok(p) => p,
        Err(message) => {
            app.toast(message, ToastKind::Error);
            return;
        }
    };
    let mut plan = app.editor.plan.clone();
    plan.clamp_to(app.editor.duration);
    let precise = app.editor.precise || !plan.is_plain_cut();
    let (rx, progress) = spawn_render(src, dst, plan, precise);
    app.editor.result = None;
    app.editor.job = Some(Job {
        rx,
        progress,
        replace,
        precise,
    });
}

fn spawn_render(
    src: PathBuf,
    dst: PathBuf,
    plan: Plan,
    precise: bool,
) -> (Receiver<Result<Done, String>>, Arc<EditProgress>) {
    let progress = Arc::new(EditProgress::default());
    let (tx, rx) = crossbeam_channel::bounded(1);
    let worker = progress.clone();
    std::thread::spawn(move || {
        let result = if precise {
            sc_ffmpeg::render_edit(&src, &dst, &plan, &worker).map(|r| Done {
                path: dst.clone(),
                secs: r.secs,
            })
        } else {
            let (a, b) = plan.bounds();
            sc_ffmpeg::trim_copy(&src, &dst, a, b).map(|secs| Done {
                path: dst.clone(),
                secs,
            })
        };
        let _ = tx.send(result.map_err(|e| e.to_string()));
    });
    (rx, progress)
}

fn poll_job(app: &mut ScApp, ctx: &egui::Context) {
    let Some(job) = &app.editor.job else {
        return;
    };
    match job.rx.try_recv() {
        Ok(Ok(done)) => {
            let replace = job.replace;
            app.editor.job = None;
            let original = app.editor.path.clone();
            finish(app, &original, done, replace);
        }
        Ok(Err(message)) => {
            app.editor.job = None;
            if message.contains(sc_ffmpeg::CANCELLED) {
                app.editor.save_open = false;
            } else {
                app.toast(message, ToastKind::Warn);
            }
        }
        Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
        Err(TryRecvError::Disconnected) => app.editor.job = None,
    }
}

fn finish(app: &mut ScApp, original: &Path, done: Done, replace: bool) {
    let Done {
        path: new_path,
        secs,
    } = done;
    app.editor.save_open = false;
    if !replace {
        app.library.set_trimmed(&new_path, true);
        if app.cfg.general.share_prompt {
            app.share = Some(crate::screens::share::ShareState::new(new_path.clone()));
        }
        app.editor.result = Some(new_path);
        app.gallery.rescan();
        app.toast(format!("Saved a {secs:.1} s clip"), ToastKind::Ok);
        return;
    }

    app.editor.stop_player();
    let mut moved = false;
    for _ in 0..8 {
        if crate::screens::gallery::delete(original) {
            moved = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    if !moved {
        let _ = std::fs::remove_file(&new_path);
        app.toast(
            "Couldn’t move the original to the Recycle Bin, so nothing was changed",
            ToastKind::Error,
        );
        app.editor.path = PathBuf::new();
        return;
    }
    let final_path = match std::fs::rename(&new_path, original) {
        Ok(()) => original.to_path_buf(),
        Err(_) => new_path,
    };
    app.library.rekey_if_missing(original, &final_path);
    app.library.set_trimmed(&final_path, true);
    if app.cfg.general.share_prompt {
        app.share = Some(crate::screens::share::ShareState::new(final_path.clone()));
    }
    app.thumbs.forget(original);
    app.thumbs.forget(&final_path);
    app.gallery.rescan();
    app.editor.path = PathBuf::new();
    app.toast(
        format!("Saved a {secs:.1} s clip. The original is in the Recycle Bin"),
        ToastKind::Ok,
    );
    app.goto(Screen::Gallery);
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testkit::{ctrl, Harness};
    use egui::{Event, Pos2};
    use sc_ffmpeg::sample::make_clip;

    fn square() -> Crop {
        Crop {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        }
    }

    #[test]
    fn a_free_corner_follows_the_pointer() {
        let c = drag_grip(square(), Grip::BottomRight, 0.9, 0.8, None);
        assert_eq!((c.x, c.y), (0.25, 0.25));
        assert!((c.w - 0.65).abs() < 1e-9 && (c.h - 0.55).abs() < 1e-9);
    }

    #[test]
    fn an_edge_only_moves_one_side() {
        let c = drag_grip(square(), Grip::Left, 0.1, 0.5, None);
        assert!((c.x - 0.1).abs() < 1e-9 && (c.w - 0.65).abs() < 1e-9);
        assert_eq!((c.y, c.h), (0.25, 0.5));
    }

    #[test]
    fn a_crop_never_collapses_or_leaves_the_picture() {
        let c = drag_grip(square(), Grip::TopLeft, 0.9, 0.9, None);
        assert!(c.w >= MIN_CROP - 1e-9 && c.h >= MIN_CROP - 1e-9);
        let c = drag_grip(square(), Grip::BottomRight, 5.0, 5.0, None);
        assert!(c.x + c.w <= 1.0 + 1e-9 && c.y + c.h <= 1.0 + 1e-9);
    }

    #[test]
    fn a_locked_corner_keeps_its_proportions_and_its_anchor() {
        let c = drag_grip(square(), Grip::BottomRight, 0.95, 0.4, Some(2.0));
        assert!((c.w / c.h - 2.0).abs() < 1e-9);
        assert_eq!((c.x, c.y), (0.25, 0.25));
        let c = drag_grip(square(), Grip::TopLeft, 0.0, 0.0, Some(2.0));
        assert!((c.x + c.w - 0.75).abs() < 1e-9 && (c.y + c.h - 0.75).abs() < 1e-9);
        assert!((c.w / c.h - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_locked_crop_stops_at_the_edge_of_the_picture() {
        let c = drag_grip(square(), Grip::BottomRight, 1.0, 1.0, Some(1.0));
        assert!(c.x + c.w <= 1.0 + 1e-9 && c.y + c.h <= 1.0 + 1e-9);
        assert!((c.w - c.h).abs() < 1e-9);
    }

    #[test]
    fn speed_labels_drop_trailing_zeros() {
        assert_eq!(trim_num(1.5), "1.5");
        assert_eq!(trim_num(2.0), "2");
        assert_eq!(trim_num(0.25), "0.25");
    }

    fn frame(st: &mut EditorState, ctx: &egui::Context, act: impl FnOnce(&mut EditorState)) {
        let before = st.plan.clone();
        act(st);
        st.commit(&before, ctx);
    }

    fn bare(duration: f64) -> EditorState {
        EditorState {
            duration,
            plan: Plan::whole(duration),
            ..Default::default()
        }
    }

    #[test]
    fn history_steps_back_and_forth() {
        let mut st = bare(10.0);
        let ctx = egui::Context::default();
        let before = st.plan.clone();
        st.plan.volume = 0.5;
        st.commit(&before, &ctx);
        assert_eq!(st.undo.len(), 1);
        st.undo();
        assert_eq!(st.plan.volume, 1.0);
        st.redo();
        assert_eq!(st.plan.volume, 0.5);
        st.reset();
        assert_eq!(st.plan, Plan::whole(10.0));
        st.undo();
        assert_eq!(st.plan.volume, 0.5);
    }

    #[test]
    fn undo_keeps_going_back_one_edit_at_a_time() {
        let mut st = bare(10.0);
        let ctx = egui::Context::default();
        frame(&mut st, &ctx, |s| s.plan.set_speed(0, 2.0));
        frame(&mut st, &ctx, |s| s.plan.volume = 0.5);
        frame(&mut st, &ctx, |s| {
            s.do_split_at(4.0);
        });
        frame(&mut st, &ctx, |s| s.plan.fade_in = 1.0);
        assert_eq!(st.undo.len(), 4);

        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan.fade_in, 0.0);
        assert_eq!(st.plan.segments.len(), 2);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan.segments.len(), 1);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan.volume, 1.0);
        assert_eq!(st.plan.segments[0].speed, 2.0);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan, Plan::whole(10.0));
        assert!(st.undo.is_empty());
        assert_eq!(st.redo.len(), 4);

        frame(&mut st, &ctx, EditorState::redo);
        frame(&mut st, &ctx, EditorState::redo);
        assert_eq!(st.plan.volume, 0.5);
        assert_eq!(st.redo.len(), 2);
        frame(&mut st, &ctx, |s| s.plan.fade_out = 2.0);
        assert!(st.redo.is_empty());
        assert_eq!(st.undo.len(), 3);
    }

    #[test]
    fn reset_is_one_undo_step() {
        let mut st = bare(10.0);
        let ctx = egui::Context::default();
        frame(&mut st, &ctx, |s| s.plan.set_speed(0, 3.0));
        frame(&mut st, &ctx, EditorState::reset);
        assert_eq!(st.undo.len(), 2);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan.segments[0].speed, 3.0);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan, Plan::whole(10.0));
    }

    #[test]
    fn the_fade_preview_follows_the_playhead() {
        let mut st = bare(10.0);
        st.plan.fade_in = 2.0;
        st.plan.fade_out = 2.0;
        assert_eq!(st.fade_now(), 0.0);
        assert_eq!(st.view().dim, 1.0);
        st.plan.fade_in = 0.0;
        st.plan.fade_out = 0.0;
        assert_eq!(st.fade_now(), 1.0);
        assert_eq!(st.view().dim, 0.0);
    }

    #[test]
    fn the_summary_lists_what_was_changed() {
        let mut st = bare(10.0);
        st.has_audio = true;
        assert!(st.summary().is_empty());
        st.plan.split(4.0);
        st.plan.set_speed(1, 2.0);
        st.plan.volume = 0.0;
        st.plan.fade_in = 1.5;
        st.plan.crop = Some(Crop::with_ratio(1920.0, 1080.0, 1.0));
        st.plan.turn = 90;
        st.plan.flip_h = true;
        let notes = st.summary().join(", ");
        for part in [
            "2 pieces",
            "speed changed on 1 of 2",
            "muted",
            "1.5s fade in",
            "cropped",
            "turned 90",
            "flipped sideways",
        ] {
            assert!(notes.contains(part), "{notes} is missing {part}");
        }
    }

    #[test]
    fn output_names_never_overwrite_an_existing_clip() {
        let dir = std::env::temp_dir().join("sc-editor-names");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_output(&dir, "sc-apex-1");
        assert_eq!(first.file_name().unwrap(), "sc-apex-1-trim.mp4");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_output(&dir, "sc-apex-1");
        assert_eq!(second.file_name().unwrap(), "sc-apex-1-trim2.mp4");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_output_goes_beside_the_clip_or_into_the_chosen_folder() {
        let dir = std::env::temp_dir().join("sc-editor-paths");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("sc-apex-1.mp4");
        let beside = output_path(&src, &None, false).unwrap();
        assert_eq!(beside, dir.join("sc-apex-1-trim.mp4"));
        let chosen = dir.join("Edited");
        let into = output_path(&src, &Some(chosen.clone()), false).unwrap();
        assert_eq!(into, chosen.join("sc-apex-1-trim.mp4"));
        assert!(chosen.is_dir());
        let replace = output_path(&src, &Some(chosen), true).unwrap();
        assert_eq!(replace, dir.join("sc-apex-1-trim.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn loaded(name: &str, secs: u32) -> Option<EditorState> {
        let dir = std::env::temp_dir().join(format!("sc-editor-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp4");
        if !make_clip(&path, secs, 320, 180, true) {
            return None;
        }
        let mut st = EditorState::default();
        st.load(&path);
        assert!(st.error.is_none(), "{:?}", st.error);
        Some(st)
    }

    struct Geo {
        left: f32,
        width: f32,
        y: f32,
        duration: f64,
    }

    impl Geo {
        fn x(&self, t: f64) -> f32 {
            self.left + (t / self.duration) as f32 * self.width
        }
        fn at(&self, t: f64) -> Pos2 {
            pos2(self.x(t), self.y)
        }
    }

    fn timeline_geo(h: &mut Harness, st: &mut EditorState) -> Geo {
        let mut origin = None;
        let mut width = 0.0;
        h.frame(Vec::new(), Modifiers::NONE, &mut |ui| {
            origin = Some(ui.available_rect_before_wrap().min);
            width = ui.available_width();
            st.timeline(ui, 0.0);
        });
        let o = origin.unwrap();
        Geo {
            left: o.x + 9.0,
            width: width - 18.0,
            y: o.y + RULER + TRACK / 2.0,
            duration: st.duration,
        }
    }

    fn timeline_gesture(h: &mut Harness, st: &mut EditorState, steps: Vec<Vec<Event>>) -> Vec<f64> {
        let mut seeks = Vec::new();
        for events in steps {
            h.frame(events, Modifiers::NONE, &mut |ui| {
                if let Some(t) = st.timeline(ui, 0.0) {
                    seeks.push(t);
                }
            });
        }
        seeks
    }

    fn click_steps(at: Pos2) -> Vec<Vec<Event>> {
        vec![
            vec![Event::PointerMoved(at)],
            vec![Harness::press(at, true)],
            vec![Harness::press(at, false)],
            Vec::new(),
        ]
    }

    fn drag_steps(from: Pos2, to: Pos2) -> Vec<Vec<Event>> {
        let mid = from + (to - from) * 0.5;
        vec![
            vec![Event::PointerMoved(from)],
            vec![Harness::press(from, true)],
            vec![Event::PointerMoved(mid)],
            vec![Event::PointerMoved(to)],
            vec![Harness::press(to, false)],
            Vec::new(),
        ]
    }

    #[test]
    fn clicking_the_timeline_moves_the_playhead_and_picks_the_piece() {
        let mut st = bare(12.0);
        st.plan.split(6.0);
        st.selected = 0;
        let mut h = Harness::new();
        let geo = timeline_geo(&mut h, &mut st);
        let seeks = timeline_gesture(&mut h, &mut st, click_steps(geo.at(9.0)));
        assert!(
            seeks.iter().any(|t| (t - 9.0).abs() < 0.2),
            "seeks {seeks:?}"
        );
        assert_eq!(st.selected, 1);
        let seeks = timeline_gesture(&mut h, &mut st, click_steps(geo.at(2.0)));
        assert!(seeks.iter().any(|t| (t - 2.0).abs() < 0.2));
        assert_eq!(st.selected, 0);
    }

    #[test]
    fn dragging_across_the_timeline_scrubs_and_lands_on_release() {
        let mut st = bare(12.0);
        let mut h = Harness::new();
        let geo = timeline_geo(&mut h, &mut st);
        let seeks = timeline_gesture(&mut h, &mut st, drag_steps(geo.at(2.0), geo.at(8.0)));
        let last = *seeks.last().expect("a seek");
        assert!((last - 8.0).abs() < 0.3, "{seeks:?}");
        assert!(st.scrub.is_none());
        assert_eq!(st.plan.segments.len(), 1);
    }

    #[test]
    fn dragging_a_handle_moves_the_edge_of_the_selected_piece() {
        let mut st = bare(12.0);
        st.plan.split(6.0);
        st.selected = 0;
        let mut h = Harness::new();
        let geo = timeline_geo(&mut h, &mut st);
        let seeks = timeline_gesture(&mut h, &mut st, drag_steps(geo.at(6.0), geo.at(4.0)));
        let seg = st.plan.segments[0];
        assert!((seg.end - 4.0).abs() < 0.3, "{seg:?}");
        assert_eq!(seg.start, 0.0);
        assert_eq!(st.plan.segments[1].start, 6.0);
        assert!(seeks.last().is_some_and(|t| (t - seg.end).abs() < 0.3));

        let start_edge = geo.at(seg.start);
        let _ = timeline_gesture(
            &mut h,
            &mut st,
            drag_steps(start_edge + vec2(2.0, 0.0), geo.at(2.0)),
        );
        assert!((st.plan.segments[0].start - 2.0).abs() < 0.4);
        assert!(st.plan.segments[0].end > 3.5);
    }

    #[test]
    fn a_handle_cannot_cross_its_neighbour() {
        let mut st = bare(12.0);
        st.plan.split(6.0);
        st.selected = 1;
        let mut h = Harness::new();
        let geo = timeline_geo(&mut h, &mut st);
        let _ = timeline_gesture(&mut h, &mut st, drag_steps(geo.at(6.0), geo.at(2.0)));
        assert_eq!(st.plan.segments[0].end, 6.0);
        assert!(st.plan.segments[1].start >= 6.0);
        assert_eq!(st.plan.segments.len(), 2);
    }

    #[test]
    fn the_split_and_delete_buttons_work() {
        let Some(mut st) = loaded("transport", 12) else {
            return;
        };
        let mut h = Harness::new();
        st.seek(4.0);
        let run = |h: &mut Harness, st: &mut EditorState| {
            h.frame(Vec::new(), Modifiers::NONE, &mut |ui| {
                transport(st, ui);
            });
        };
        run(&mut h, &mut st);
        assert!(h.has("Split here") && h.has("Delete piece"));

        let mut message = None;
        h.click_text("Split here", &mut |ui| {
            message = message.or(transport(&mut st, ui));
        });
        assert_eq!(st.plan.segments.len(), 2);
        assert_eq!(st.plan.segments[0].end, st.plan.segments[1].start);
        assert!((st.plan.segments[0].end - 4.0).abs() < 0.05);
        assert_eq!(st.selected, 1);
        assert!(message.is_none());

        let mut message = None;
        h.click_text("Split here", &mut |ui| {
            message = message.or(transport(&mut st, ui));
        });
        assert_eq!(
            st.plan.segments.len(),
            2,
            "no second split at the same spot"
        );
        assert!(message.is_some());

        h.click_text("Delete piece", &mut |ui| {
            transport(&mut st, ui);
        });
        assert_eq!(st.plan.segments.len(), 1);
        assert_eq!(st.plan.segments[0].end, 4.0);
        assert_eq!(st.selected, 0);
    }

    #[test]
    fn the_keyboard_shortcuts_work() {
        let Some(mut st) = loaded("keys", 12) else {
            return;
        };
        let mut h = Harness::new();
        let ctx = h.ctx.clone();
        let press = |h: &mut Harness, st: &mut EditorState, key: Key, m: Modifiers| {
            let mut note = None;
            h.key(key, m, &mut |_| {
                note = note.or(handle_keys(st, &ctx));
            });
            note
        };

        st.seek(4.0);
        assert!(press(&mut h, &mut st, Key::S, Modifiers::NONE).is_none());
        assert_eq!(st.plan.segments.len(), 2);
        assert!(press(&mut h, &mut st, Key::S, Modifiers::NONE).is_some());

        press(&mut h, &mut st, Key::ArrowRight, Modifiers::NONE);
        assert!((st.position() - 5.0).abs() < 0.05, "{}", st.position());
        press(&mut h, &mut st, Key::ArrowRight, Modifiers::SHIFT);
        assert!((st.position() - 10.0).abs() < 0.05, "{}", st.position());
        press(&mut h, &mut st, Key::ArrowLeft, Modifiers::NONE);
        assert!((st.position() - 9.0).abs() < 0.05);

        assert!(!st.player.as_ref().unwrap().is_playing());
        press(&mut h, &mut st, Key::Space, Modifiers::NONE);
        assert!(st.player.as_ref().unwrap().is_playing());
        press(&mut h, &mut st, Key::Space, Modifiers::NONE);
        assert!(!st.player.as_ref().unwrap().is_playing());

        assert_eq!(st.selected, 1);
        assert!(press(&mut h, &mut st, Key::Delete, Modifiers::NONE).is_none());
        assert_eq!(st.plan.segments.len(), 1);
        assert!(press(&mut h, &mut st, Key::Delete, Modifiers::NONE).is_some());

        st.undo.push(st.plan.clone());
        st.plan.volume = 0.3;
        press(&mut h, &mut st, Key::Z, ctrl());
        assert_eq!(st.plan.volume, 1.0);
        press(&mut h, &mut st, Key::Y, ctrl());
        assert_eq!(st.plan.volume, 0.3);
        press(&mut h, &mut st, Key::Z, ctrl());
        assert_eq!(st.plan.volume, 1.0);
        let shift_ctrl = Modifiers {
            shift: true,
            ..ctrl()
        };
        press(&mut h, &mut st, Key::Z, shift_ctrl);
        assert_eq!(st.plan.volume, 0.3);

        assert!(!st.save_open);
        press(&mut h, &mut st, Key::S, ctrl());
        assert!(st.save_open);
    }

    fn run_tools(h: &mut Harness, st: &mut EditorState) {
        h.frame(Vec::new(), Modifiers::NONE, &mut |ui| tools(st, ui));
    }

    fn choose_tool(h: &mut Harness, st: &mut EditorState, tab: &str) {
        run_tools(h, st);
        h.click_text(tab, &mut |ui| tools(st, ui));
    }

    fn slider_centre(label: Rect) -> Pos2 {
        pos2(label.right() - 56.0 - 8.0 - 90.0, label.center().y)
    }

    #[test]
    fn speed_is_set_per_piece_from_the_chips() {
        let mut st = bare(12.0);
        st.plan.split(4.0);
        st.plan.split(8.0);
        st.selected = 1;
        let mut h = Harness::new();
        run_tools(&mut h, &mut st);
        assert!(h.has("Speed of piece 2 of 3"));

        h.click_text("2\u{d7}", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.segments[1].speed, 2.0);
        assert_eq!(st.plan.segments[0].speed, 1.0);
        assert_eq!(st.plan.segments[2].speed, 1.0);

        st.selected = 2;
        run_tools(&mut h, &mut st);
        assert!(h.has("Speed of piece 3 of 3"));
        h.click_text("0.5\u{d7}", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.segments[2].speed, 0.5);
        assert_eq!(st.plan.segments[1].speed, 2.0);

        assert!(h.has("Apply to all"));
        h.click_text("Apply to all", &mut |ui| tools(&mut st, ui));
        assert!(st.plan.segments.iter().all(|s| s.speed == 0.5));
        run_tools(&mut h, &mut st);
        assert!(!h.has("Apply to all"));
    }

    #[test]
    fn a_single_piece_clip_has_one_speed() {
        let mut st = bare(12.0);
        let mut h = Harness::new();
        run_tools(&mut h, &mut st);
        assert!(h.has("Speed") && !h.has("Apply to all"));
        h.click_text("4\u{d7}", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.segments[0].speed, 4.0);
        h.click_text("1\u{d7}", &mut |ui| tools(&mut st, ui));
        assert!(!st.plan.has_speed_change());
    }

    #[test]
    fn the_sound_tool_sets_volume_and_mutes() {
        let mut st = bare(12.0);
        st.has_audio = true;
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Sound");
        assert_eq!(st.tool, Tool::Sound);
        assert!(h.has("Volume") && h.has("Mute") && h.has("100%"));

        let label = h.at("100%", 0).unwrap();
        let centre = slider_centre(label);
        h.drag(
            centre + vec2(10.0, 0.0),
            centre + vec2(-80.0, 0.0),
            &mut |ui| tools(&mut st, ui),
        );
        assert!(
            st.plan.volume > 0.0 && st.plan.volume < 0.5,
            "{}",
            st.plan.volume
        );
        assert_eq!(st.saved_volume, st.plan.volume);

        let label = h.at(&format!("{:.0}%", st.plan.volume * 100.0), 0).unwrap();
        let centre = slider_centre(label);
        h.drag(
            centre + vec2(-60.0, 0.0),
            centre + vec2(140.0, 0.0),
            &mut |ui| tools(&mut st, ui),
        );
        assert!((st.plan.volume - 2.0).abs() < 1e-6, "{}", st.plan.volume);

        let loud = st.plan.volume;
        let toggle = pos2(h.right - 16.0 - 25.5, h.at("Mute", 0).unwrap().center().y);
        h.click(toggle, &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.volume, 0.0);
        run_tools(&mut h, &mut st);
        assert!(h.has("Muted"));
        h.click(toggle, &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.volume, loud);
    }

    #[test]
    fn the_sound_timing_nudges_and_slides() {
        let mut st = bare(12.0);
        st.has_audio = true;
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Sound");
        assert!(h.has("Sound timing") && h.has("In sync"));

        h.click_text("Later", &mut |ui| tools(&mut st, ui));
        assert!(
            (st.plan.audio_shift - 0.01).abs() < 1e-9,
            "{}",
            st.plan.audio_shift
        );
        h.click_text("Later", &mut |ui| tools(&mut st, ui));
        h.click_text("Later", &mut |ui| tools(&mut st, ui));
        assert!((st.plan.audio_shift - 0.03).abs() < 1e-9);
        run_tools(&mut h, &mut st);
        assert!(h.has("+30 ms"));

        for _ in 0..5 {
            h.click_text("Earlier", &mut |ui| tools(&mut st, ui));
        }
        assert!(
            (st.plan.audio_shift + 0.02).abs() < 1e-9,
            "{}",
            st.plan.audio_shift
        );
        run_tools(&mut h, &mut st);
        assert!(h.has("-20 ms"));
        assert!(st.summary().iter().any(|n| n == "sound 20 ms earlier"));

        let label = h.at("-20 ms", 0).unwrap();
        let centre = slider_centre(label);
        h.drag(
            centre + vec2(-20.0, 0.0),
            centre + vec2(150.0, 0.0),
            &mut |ui| tools(&mut st, ui),
        );
        assert_eq!(st.plan.audio_shift, AUDIO_SHIFT_MAX);
        assert!(st.summary().iter().any(|n| n == "sound 3000 ms later"));
        let label = h.at("+3000 ms", 0).unwrap();
        let centre = slider_centre(label);
        h.drag(
            centre + vec2(60.0, 0.0),
            centre + vec2(-150.0, 0.0),
            &mut |ui| tools(&mut st, ui),
        );
        assert_eq!(st.plan.audio_shift, -AUDIO_SHIFT_MAX);
    }

    #[test]
    fn the_shift_stops_at_its_limits_and_counts_as_a_change() {
        let mut st = bare(12.0);
        st.has_audio = true;
        st.plan.audio_shift = AUDIO_SHIFT_MAX - 0.004;
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Sound");
        h.click_text("Later", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.audio_shift, AUDIO_SHIFT_MAX);
        assert!(!st.plan.is_plain_cut());
    }

    #[test]
    fn a_clip_without_sound_says_so() {
        let mut st = bare(12.0);
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Sound");
        assert!(h.has("No sound"));
        assert!(!h.has("Volume"));
    }

    #[test]
    fn the_fade_sliders_set_both_ends() {
        let mut st = bare(12.0);
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Fades");
        assert_eq!(st.tool, Tool::Fades);
        assert!(h.has("Fade in") && h.has("Fade out"));

        let first = h.at("Off", 0).unwrap();
        let centre = slider_centre(first);
        h.click(centre + vec2(-20.0, 0.0), &mut |ui| tools(&mut st, ui));
        h.click(centre + vec2(-20.0, 0.0), &mut |ui| tools(&mut st, ui));
        assert!(
            st.plan.fade_in > 1.0 && st.plan.fade_in < 3.0,
            "{}",
            st.plan.fade_in
        );
        assert_eq!(st.plan.fade_out, 0.0);

        let second = h.at("Off", 0).unwrap();
        let centre = slider_centre(second);
        h.click(centre + vec2(60.0, 0.0), &mut |ui| tools(&mut st, ui));
        assert!(st.plan.fade_out > 3.0, "{}", st.plan.fade_out);

        let at = h
            .at(&format!("{} s", trim_num(st.plan.fade_in)), 0)
            .unwrap();
        let centre = slider_centre(at);
        h.drag(
            centre + vec2(-20.0, 0.0),
            centre + vec2(-120.0, 0.0),
            &mut |ui| tools(&mut st, ui),
        );
        assert_eq!(st.plan.fade_in, 0.0);
    }

    #[test]
    fn opening_the_crop_tab_does_not_crop_anything() {
        let mut st = bare(12.0);
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Crop");
        assert_eq!(st.tool, Tool::Crop);
        for _ in 0..3 {
            run_tools(&mut h, &mut st);
        }
        assert!(st.plan.crop.is_none());
        assert!(st.shape.is_none());
    }

    #[test]
    fn the_crop_shapes_and_clear_work() {
        let Some(mut st) = loaded("crop", 6) else {
            return;
        };
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Crop");

        h.click_text("1:1", &mut |ui| tools(&mut st, ui));
        let c = st.plan.crop.expect("a crop");
        assert!((c.w - 0.5625).abs() < 1e-6 && c.h == 1.0, "{c:?}");
        assert_eq!(st.shape, Some(Shape::Square));

        h.click_text("9:16", &mut |ui| tools(&mut st, ui));
        let c = st.plan.crop.unwrap();
        assert!(
            (c.w * 320.0 / (c.h * 180.0) - 9.0 / 16.0).abs() < 1e-6,
            "{c:?}"
        );

        let locked = st.plan.crop.unwrap();
        h.click_text("Free", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.crop, Some(locked), "unlocking keeps the same frame");
        assert_eq!(st.shape, Some(Shape::Free));

        assert!(h.has("Clear"));
        h.click_text("Clear", &mut |ui| tools(&mut st, ui));
        assert!(st.plan.crop.is_none());
        assert!(st.shape.is_none());
        run_tools(&mut h, &mut st);
        assert!(!h.has("Clear"));

        h.click_text("Free", &mut |ui| tools(&mut st, ui));
        let c = st.plan.crop.expect("a free crop to start from");
        assert!(c.w < 1.0 && c.h < 1.0 && c.x > 0.0 && c.y > 0.0, "{c:?}");
    }

    #[test]
    fn the_rotate_tool_turns_and_flips() {
        let mut st = bare(12.0);
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Rotate");
        assert_eq!(st.tool, Tool::Rotate);

        h.click_text("Turn right", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.turn, 90);
        h.click_text("Turn right", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.turn, 180);
        h.click_text("Turn left", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.turn, 90);
        h.click_text("Turn left", &mut |ui| tools(&mut st, ui));
        h.click_text("Turn left", &mut |ui| tools(&mut st, ui));
        assert_eq!(st.plan.turn, 270);

        h.click_text("Sideways", &mut |ui| tools(&mut st, ui));
        assert!(st.plan.flip_h && !st.plan.flip_v);
        assert!(h.has("Sideways: on"));
        h.click_text("Upside down", &mut |ui| tools(&mut st, ui));
        assert!(st.plan.flip_v);
        h.click_text("Sideways: on", &mut |ui| tools(&mut st, ui));
        assert!(!st.plan.flip_h);
        assert_eq!(st.view().turn, 270);
        assert!(st.view().flip_v);
    }

    #[test]
    fn turning_clears_a_crop_that_no_longer_fits() {
        let mut st = bare(12.0);
        st.plan.crop = Some(square());
        st.shape = Some(Shape::Free);
        let mut h = Harness::new();
        choose_tool(&mut h, &mut st, "Rotate");
        h.click_text("Turn right", &mut |ui| tools(&mut st, ui));
        assert!(st.plan.crop.is_none());
        assert!(st.shape.is_none());
    }

    #[test]
    fn every_tool_tab_shows_its_own_controls() {
        let mut st = bare(12.0);
        st.has_audio = true;
        let mut h = Harness::new();
        for (tab, tool, marker) in [
            ("Sound", Tool::Sound, "Volume"),
            ("Fades", Tool::Fades, "Fade in"),
            ("Crop", Tool::Crop, "Shape"),
            ("Rotate", Tool::Rotate, "Flip"),
            (
                "Speed",
                Tool::Speed,
                "Split the clip to slow down or speed up just one part. Sound keeps its pitch.",
            ),
        ] {
            choose_tool(&mut h, &mut st, tab);
            assert_eq!(st.tool, tool);
            assert!(h.has(marker), "{tab} is missing {marker}");
        }
    }

    #[test]
    fn the_save_dialog_offers_the_choices_and_starts() {
        let mut st = bare(12.0);
        st.precise = true;
        st.save_open = true;
        let mut h = Harness::new();
        let mut act = SaveAction::Idle;
        let run = |h: &mut Harness, st: &mut EditorState, act: &mut SaveAction| {
            h.frame(Vec::new(), Modifiers::NONE, &mut |ui| {
                *act = save_dialog(st, &None, ui.ctx());
            });
        };
        run(&mut h, &mut st, &mut act);
        run(&mut h, &mut st, &mut act);
        for text in [
            "Save your edit",
            "Frame accurate",
            "Save as a copy",
            "Replace the original",
            "Save edited clip",
            "Cancel",
        ] {
            assert!(h.has(text), "dialog is missing {text}");
        }

        h.click_text("Replace the original", &mut |ui| {
            act = save_dialog(&mut st, &None, ui.ctx());
        });
        assert_eq!(st.mode, SaveMode::Replace);
        assert!(h.has("Replace original"));

        h.click_text("Save as a copy", &mut |ui| {
            act = save_dialog(&mut st, &None, ui.ctx());
        });
        assert_eq!(st.mode, SaveMode::Copy);

        let mut started = false;
        h.click_text("Save edited clip", &mut |ui| {
            started |= save_dialog(&mut st, &None, ui.ctx()) == SaveAction::Start;
        });
        assert!(started);
        assert!(st.save_open);

        h.click_text("Cancel", &mut |ui| {
            act = save_dialog(&mut st, &None, ui.ctx());
        });
        assert!(!st.save_open);
    }

    #[test]
    fn the_save_dialog_hides_the_accuracy_switch_for_real_edits() {
        let mut st = bare(12.0);
        st.has_audio = true;
        st.save_open = true;
        st.plan.volume = 0.5;
        let mut h = Harness::new();
        for _ in 0..2 {
            h.frame(Vec::new(), Modifiers::NONE, &mut |ui| {
                save_dialog(&mut st, &None, ui.ctx());
            });
        }
        assert!(h.has("Save your edit"));
        assert!(!h.has("Frame accurate"));
        assert!(h.texts.iter().any(|(t, _)| t.contains("50% volume")));
    }

    #[test]
    fn escape_closes_the_save_dialog() {
        let mut st = bare(12.0);
        st.save_open = true;
        let mut h = Harness::new();
        h.key(Key::Escape, Modifiers::NONE, &mut |ui| {
            save_dialog(&mut st, &None, ui.ctx());
        });
        assert!(!st.save_open);
    }

    fn wait_for(rx: &Receiver<Result<Done, String>>) -> Result<Done, String> {
        rx.recv_timeout(Duration::from_secs(60)).expect("a result")
    }

    #[test]
    fn saving_a_plain_cut_exactly_gives_the_chosen_length() {
        let Some(st) = loaded("save-plain", 8) else {
            return;
        };
        let dst = st.path.with_file_name("exact.mp4");
        let mut plan = Plan::whole(8.0);
        plan.segments = vec![sc_core::edit::Segment::new(1.2, 4.2)];
        assert!(plan.is_plain_cut());
        let (rx, progress) = spawn_render(st.path.clone(), dst.clone(), plan, true);
        let done = wait_for(&rx).expect("saved");
        assert_eq!(done.path, dst);
        assert!((done.secs - 3.0).abs() < 0.2, "{}", done.secs);
        let info = sc_ffmpeg::media_info(&dst).unwrap();
        assert!((info.duration - 3.0).abs() < 0.25, "{}", info.duration);
        assert_eq!(progress.fraction(), 1.0);
    }

    #[test]
    fn saving_a_plain_cut_without_re_encoding_still_works() {
        let Some(st) = loaded("save-lossless", 8) else {
            return;
        };
        let dst = st.path.with_file_name("fast.mp4");
        let mut plan = Plan::whole(8.0);
        plan.segments = vec![sc_core::edit::Segment::new(2.0, 6.0)];
        let (rx, _) = spawn_render(st.path.clone(), dst.clone(), plan, false);
        let done = wait_for(&rx).expect("saved");
        assert!(done.path.exists());
        let info = sc_ffmpeg::media_info(&dst).unwrap();
        assert!(
            info.duration > 3.5 && info.duration < 6.1,
            "{}",
            info.duration
        );
        assert_eq!(info.audio_streams, 1);
    }

    #[test]
    fn saving_every_kind_of_edit_at_once_works() {
        let Some(st) = loaded("save-everything", 8) else {
            return;
        };
        let dst = st.path.with_file_name("all.mp4");
        let mut plan = Plan::whole(8.0);
        plan.segments = vec![
            sc_core::edit::Segment {
                start: 0.5,
                end: 3.0,
                speed: 0.5,
            },
            sc_core::edit::Segment {
                start: 4.0,
                end: 8.0,
                speed: 2.0,
            },
        ];
        plan.volume = 1.5;
        plan.fade_in = 0.5;
        plan.fade_out = 0.5;
        plan.turn = 270;
        plan.flip_h = true;
        plan.crop = Some(Crop::with_ratio(180.0, 320.0, 1.0));
        let want = plan.output_secs();
        let (w, h) = plan.output_size(320, 180);
        let (rx, _) = spawn_render(st.path.clone(), dst.clone(), plan, true);
        let done = wait_for(&rx).expect("saved");
        assert!((done.secs - want).abs() < 0.01);
        let info = sc_ffmpeg::media_info(&dst).unwrap();
        assert_eq!((info.width, info.height), (w, h));
        assert!(
            (info.duration - want).abs() < 0.4,
            "{} vs {want}",
            info.duration
        );
        assert_eq!(info.audio_streams, 1);
    }

    #[test]
    fn a_failed_save_reports_the_problem() {
        let dir = std::env::temp_dir().join("sc-editor-missing");
        let _ = std::fs::create_dir_all(&dir);
        let (rx, _) = spawn_render(
            dir.join("gone.mp4"),
            dir.join("out.mp4"),
            Plan::whole(5.0),
            true,
        );
        let err = wait_for(&rx).expect_err("an error");
        assert!(!err.is_empty());
    }

    #[test]
    fn loading_something_that_is_not_a_video_shows_an_error() {
        let dir = std::env::temp_dir().join("sc-editor-bad");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("not-a-video.mp4");
        std::fs::write(&path, b"nothing to see").unwrap();
        let mut st = EditorState::default();
        st.load(&path);
        assert!(st.error.is_some());
        assert!(st.player.is_none() || st.error.is_some());
    }

    #[test]
    fn a_loaded_clip_starts_whole_and_untouched() {
        let Some(st) = loaded("fresh", 6) else {
            return;
        };
        assert!(st.error.is_none());
        assert!((st.duration - 6.0).abs() < 0.2);
        assert!(st.has_audio);
        assert_eq!(st.plan.segments.len(), 1);
        assert!(st.plan.is_plain_cut());
        assert!(st.precise);
        assert!(st.undo.is_empty() && st.redo.is_empty());
        assert!(st.summary().is_empty());
    }
}
