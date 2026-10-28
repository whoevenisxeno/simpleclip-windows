use crate::app::{ScApp, Screen, ToastKind};
use crate::player::{Player, View};
use crate::theme::{self, icon, pal_ui};
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{pos2, vec2, Color32, Key, Modifiers, Rect, RichText, Rounding, Sense, Stroke, Ui};
use sc_core::edit::{Crop, Plan, MIN_CROP};
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

#[derive(Clone, Copy, PartialEq, Default)]
enum SaveMode {
    #[default]
    Copy,
    Replace,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Tool {
    #[default]
    Speed,
    Sound,
    Fades,
    Crop,
    Rotate,
}

#[derive(Clone, Copy, PartialEq)]
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

#[derive(Clone, Copy, PartialEq)]
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
            self.plan.speed = 1.5;
        }
        self.selected = 0;
        self.plan.crop = None;
        self.plan.turn = 0;
        self.plan.flip_h = false;
        self.tool = Tool::Speed;
        match name {
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
                player.set_segments(vec![(0.0, i.duration)]);
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
        if (self.plan.speed - 1.0).abs() > 1e-6 {
            out.push(format!("{}\u{d7} speed", trim_num(self.plan.speed)));
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
    let (mut undo, mut redo, mut reset) = (false, false, false);
    ui.horizontal(|ui| {
        if widgets::quiet_button(ui, Some(icon::BACK), "Clips").clicked() {
            leave = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let st = &app.editor;
            ui.add_enabled_ui(!busy && st.error.is_none(), |ui| {
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

    let before = app.editor.plan.clone();
    if !busy {
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
            st.plan.segments.iter().map(|s| (s.start, s.end)).collect(),
            st.plan.volume,
        )
    };
    let view_state = app.editor.view();
    let crop_tool = app.editor.tool == Tool::Crop;
    let mut toggle_play = false;
    if let Some(pl) = app.editor.player.as_mut() {
        pl.set_segments(segs);
        pl.set_gain(gain);
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
        if (st.plan.speed - 1.0).abs() > 1e-6 {
            speed_badge(ui, rect, st.plan.speed);
        }
    }
    if toggle_play {
        if let Some(pl) = app.editor.player.as_mut() {
            pl.toggle();
        }
    }
    ui.add_space(10.0);

    transport(app, ui);
    ui.add_space(8.0);
    tools(app, ui);
    export_card(app, ui);

    let st = &mut app.editor;
    st.commit(&before, &ctx);
    if st.plan != before {
        ctx.request_repaint();
    }
}

fn keys(app: &mut ScApp, ctx: &egui::Context) {
    if ctx.wants_keyboard_input() {
        return;
    }
    let st = &mut app.editor;
    let (undo, redo, space, split, delete, left, right, shift) = ctx.input_mut(|i| {
        let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)
            || i.consume_key(Modifiers::COMMAND, Key::Y);
        let undo = i.consume_key(Modifiers::COMMAND, Key::Z);
        (
            undo,
            redo,
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
    if space {
        if let Some(p) = st.player.as_mut() {
            p.toggle();
        }
    }
    if split && !st.do_split() {
        app.toast(
            "Move the playhead inside a piece to split it",
            ToastKind::Warn,
        );
        return;
    }
    if delete && !st.do_delete() {
        app.toast("A clip needs at least one piece", ToastKind::Warn);
        return;
    }
    let step = if shift { 5.0 } else { 1.0 };
    if left || right {
        let to = (st.position() + if left { -step } else { step }).clamp(0.0, st.duration);
        st.seek(to);
    }
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

fn transport(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let mut play = false;
    let mut split = false;
    let mut delete = false;
    let st = &mut app.editor;
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
        let msg = if (kept - st.duration).abs() < 0.05 && (out - kept).abs() < 0.05 {
            "Click to move the playhead. S splits, Delete removes the selected piece.".to_string()
        } else {
            format!(
                "Result {} from {} kept of {}.",
                clock(out),
                clock(kept),
                clock(st.duration)
            )
        };
        ui.label(RichText::new(msg).size(12.0).color(p.text_3));
    });
    if play {
        if let Some(pl) = st.player.as_mut() {
            pl.toggle();
        }
    }
    if split && !st.do_split() {
        app.toast(
            "Move the playhead inside a piece to split it",
            ToastKind::Warn,
        );
    } else if delete && !st.do_delete() {
        app.toast("A clip needs at least one piece", ToastKind::Warn);
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

fn tools(app: &mut ScApp, ui: &mut Ui) {
    let st = &mut app.editor;
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
    widgets::section(ui, None, |s| {
        s.row(
            "Playback speed",
            Some("Sound keeps its pitch. The preview plays at normal speed."),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                widgets::segmented(ui, &mut st.plan.speed, &SPEEDS);
            },
        );
    });
}

fn sound_tool(st: &mut EditorState, ui: &mut Ui) {
    let p = pal_ui(ui);
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
                ui.label(
                    RichText::new(if pct == 0 {
                        "Muted".to_string()
                    } else {
                        format!("{pct}%")
                    })
                    .size(13.0)
                    .color(p.text_2),
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
    let p = pal_ui(ui);
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
                ui.label(
                    RichText::new(label(st.plan.fade_in))
                        .size(13.0)
                        .color(p.text_2),
                );
                if widgets::step_slider(ui, &mut tenths, 50, 0).changed() {
                    st.plan.fade_in = tenths as f64 / 10.0;
                }
            },
        );
        s.row("Fade out", Some("Ends in black and silence."), |ui| {
            let mut tenths = (st.plan.fade_out * 10.0).round() as u32;
            ui.label(
                RichText::new(label(st.plan.fade_out))
                    .size(13.0)
                    .color(p.text_2),
            );
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

fn export_card(app: &mut ScApp, ui: &mut Ui) {
    let p = pal_ui(ui);
    let mut save = false;
    let mut cancel = false;
    let trim_dir = app.cfg.save.trim_directory.clone();
    let st = &mut app.editor;
    let plain = st.plan.is_plain_cut();
    let notes = st.summary();
    let running = st.job.as_ref().map(|j| j.progress.fraction());
    ui.add_space(10.0);
    widgets::card(ui, |ui| {
        let (sw, sh) = st.player.as_ref().map(Player::size).unwrap_or((16, 9));
        let (ow, oh) = st.plan.output_size(sw, sh);
        let line = if notes.is_empty() {
            format!(
                "No changes yet. {} \u{b7} {ow} \u{d7} {oh}",
                clock(st.plan.output_secs())
            )
        } else {
            format!(
                "{} \u{b7} {ow} \u{d7} {oh} \u{b7} {}",
                clock(st.plan.output_secs()),
                notes.join(", ")
            )
        };
        ui.label(RichText::new(line).size(13.0).color(p.text));
        ui.add_space(10.0);
        if let Some(f) = running {
            let precise = st.job.as_ref().is_some_and(|j| j.precise);
            let replace = st.job.as_ref().is_some_and(|j| j.replace);
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 110.0).max(80.0);
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
                cancel = widgets::quiet_button(ui, None, "Cancel").clicked();
            });
            ui.add_space(4.0);
            widgets::secondary(
                ui,
                if precise {
                    if replace {
                        "Rendering the edit. The original stays until it is done."
                    } else {
                        "Rendering the edit."
                    }
                } else {
                    "Cutting the clip."
                },
            );
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
                            "Re-encodes the clip so the cut lands exactly where you put it."
                        } else {
                            "An instant lossless cut. It can start up to about 2 s early, on the nearest keyframe."
                        })
                        .size(12.0)
                        .color(p.text_2),
                    );
                });
            });
            ui.add_space(10.0);
        }
        ui.horizontal(|ui| {
            widgets::segmented(
                ui,
                &mut st.mode,
                &[
                    (SaveMode::Copy, "Save as a copy"),
                    (SaveMode::Replace, "Replace the original"),
                ],
            );
        });
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
        ui.add_space(10.0);
        let label = match st.mode {
            SaveMode::Copy => "Save edited clip",
            SaveMode::Replace => "Replace original",
        };
        save = widgets::primary_button(ui, Some(icon::CUT), label).clicked();
    });
    if cancel {
        if let Some(j) = &app.editor.job {
            j.progress.cancel();
        }
    }
    if save {
        start_export(app, trim_dir);
    }

    if let Some(r) = app.editor.result.clone() {
        ui.add_space(10.0);
        let mut see = false;
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Saved {}",
                        r.file_name().and_then(|n| n.to_str()).unwrap_or("copy")
                    ))
                    .color(p.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    see = widgets::quiet_button(ui, None, "See edited clips").clicked();
                });
            });
        });
        if see {
            app.gallery.show_trimmed();
            app.goto(Screen::Gallery);
        }
    }
    ui.add_space(8.0);
    widgets::secondary(
        ui,
        match &app.cfg.save.trim_directory {
            Some(d) => format!("Edited clips are saved to {}.", d.display()),
            None => "Edited clips are saved next to the original. Pick a folder in Settings."
                .to_string(),
        },
    );
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

fn start_export(app: &mut ScApp, trim_dir: Option<PathBuf>) {
    let replace = app.editor.mode == SaveMode::Replace;
    let src = app.editor.path.clone();
    let Some(stem) = src.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
        app.toast("That file name can’t be edited", ToastKind::Error);
        return;
    };
    let beside = src.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = match (&trim_dir, replace) {
        (Some(d), false) => match std::fs::create_dir_all(d) {
            Ok(()) => d.clone(),
            Err(e) => {
                app.toast(
                    format!("Can’t create the folder {}: {e}", d.display()),
                    ToastKind::Error,
                );
                return;
            }
        },
        _ => beside,
    };
    let dst = unique_output(&dir, &stem);
    let mut plan = app.editor.plan.clone();
    plan.clamp_to(app.editor.duration);
    let precise = app.editor.precise || !plan.is_plain_cut();
    let progress = Arc::new(EditProgress::default());
    let (tx, rx) = crossbeam_channel::bounded(1);
    let worker = progress.clone();
    let out = dst.clone();
    std::thread::spawn(move || {
        let result = if precise {
            sc_ffmpeg::render_edit(&src, &out, &plan, &worker).map(|r| Done {
                path: out.clone(),
                secs: r.secs,
            })
        } else {
            let (a, b) = plan.bounds();
            sc_ffmpeg::trim_copy(&src, &out, a, b).map(|secs| Done {
                path: out.clone(),
                secs,
            })
        };
        let _ = tx.send(result.map_err(|e| e.to_string()));
    });
    app.editor.result = None;
    app.editor.job = Some(Job {
        rx,
        progress,
        replace,
        precise,
    });
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
            if !message.contains(sc_ffmpeg::CANCELLED) {
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
    if !crate::screens::gallery::delete(original) {
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

    #[test]
    fn history_steps_back_and_forth() {
        let mut st = EditorState {
            duration: 10.0,
            plan: Plan::whole(10.0),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let before = st.plan.clone();
        st.plan.speed = 2.0;
        st.commit(&before, &ctx);
        assert_eq!(st.undo.len(), 1);
        st.undo();
        assert_eq!(st.plan.speed, 1.0);
        st.redo();
        assert_eq!(st.plan.speed, 2.0);
        st.reset();
        assert_eq!(st.plan, Plan::whole(10.0));
        st.undo();
        assert_eq!(st.plan.speed, 2.0);
    }

    fn frame(st: &mut EditorState, ctx: &egui::Context, act: impl FnOnce(&mut EditorState)) {
        let before = st.plan.clone();
        act(st);
        st.commit(&before, ctx);
    }

    #[test]
    fn undo_keeps_going_back_one_edit_at_a_time() {
        let mut st = EditorState {
            duration: 10.0,
            plan: Plan::whole(10.0),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        frame(&mut st, &ctx, |s| s.plan.speed = 2.0);
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
        assert_eq!(st.plan.speed, 2.0);
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
        let mut st = EditorState {
            duration: 10.0,
            plan: Plan::whole(10.0),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        frame(&mut st, &ctx, |s| s.plan.speed = 3.0);
        frame(&mut st, &ctx, EditorState::reset);
        assert_eq!(st.undo.len(), 2);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan.speed, 3.0);
        frame(&mut st, &ctx, EditorState::undo);
        assert_eq!(st.plan, Plan::whole(10.0));
    }
}
