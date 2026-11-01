use crate::theme::{self, mix, pal_ui, Palette};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    pos2, vec2, Align2, Color32, ColorImage, CursorIcon, FontId, Id, Pos2, Rect, Rounding, Sense,
    Stroke, TextureFilter, TextureHandle, TextureOptions, Ui,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub const SCREEN: [f32; 2] = [1920.0, 1080.0];
pub const BOX: [f32; 2] = [384.0, 144.0];
pub const WIDTH: f32 = 360.0;
pub const SNAP: f32 = 6.0;
pub const CHECK_EVERY: Duration = Duration::from_millis(500);
const EPS: f32 = 0.0015;
const PAD_W: f32 = 104.0;
const SIDE: f32 = 2.0 * PAD_W + 8.0;

#[derive(Clone, Debug)]
pub enum Picture {
    File(PathBuf),
    Made(Arc<ColorImage>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub const CORNERS: [(Corner, &str); 4] = [
    (Corner::TopLeft, "Top left"),
    (Corner::TopRight, "Top right"),
    (Corner::BottomLeft, "Bottom left"),
    (Corner::BottomRight, "Bottom right"),
];

pub fn plain(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != ".."
        && name.to_ascii_lowercase().ends_with(".png")
}

pub fn file_of(dir: &Path, name: &str) -> Option<PathBuf> {
    plain(name).then(|| dir.join(name))
}

pub fn share(px: [f32; 2]) -> [f32; 2] {
    [
        (px[0] / SCREEN[0]).clamp(0.0, 1.0),
        (px[1] / SCREEN[1]).clamp(0.0, 1.0),
    ]
}

pub fn to_px(screen: Rect, p: [f32; 2]) -> Pos2 {
    screen.min + vec2(p[0] * screen.width(), p[1] * screen.height())
}

pub fn to_frac(screen: Rect, at: Pos2) -> [f32; 2] {
    let d = at - screen.min;
    [d.x / screen.width(), d.y / screen.height()]
}

pub fn clamp(p: [f32; 2], s: [f32; 2]) -> [f32; 2] {
    let f = |v: f32, s: f32| v.clamp(0.0, (1.0 - s).max(0.0));
    [f(p[0], s[0]), f(p[1], s[1])]
}

fn snap_axis(v: f32, s: f32, tol: f32) -> (f32, Option<f32>) {
    [(0.0, 0.0), ((1.0 - s) / 2.0, 0.5), (1.0 - s, 1.0)]
        .into_iter()
        .map(|(at, line)| ((v - at).abs(), at, line))
        .filter(|(d, ..)| *d <= tol)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or((v, None), |(_, at, line)| (at.max(0.0), Some(line)))
}

pub fn snap(p: [f32; 2], s: [f32; 2], tol: [f32; 2]) -> ([f32; 2], [Option<f32>; 2]) {
    let (x, gx) = snap_axis(p[0], s[0], tol[0]);
    let (y, gy) = snap_axis(p[1], s[1], tol[1]);
    ([x, y], [gx, gy])
}

pub fn corner(c: Corner, s: [f32; 2]) -> [f32; 2] {
    let (r, b) = ((1.0 - s[0]).max(0.0), (1.0 - s[1]).max(0.0));
    match c {
        Corner::TopLeft => [0.0, 0.0],
        Corner::TopRight => [r, 0.0],
        Corner::BottomLeft => [0.0, b],
        Corner::BottomRight => [r, b],
    }
}

fn side(v: f32, s: f32) -> Option<u8> {
    let end = (1.0 - s).max(0.0);
    if v.abs() <= EPS {
        Some(0)
    } else if (v - end).abs() <= EPS {
        Some(2)
    } else if (v - end / 2.0).abs() <= EPS {
        Some(1)
    } else {
        None
    }
}

pub fn caption(p: [f32; 2], s: [f32; 2]) -> String {
    let named = match (side(p[0], s[0]), side(p[1], s[1])) {
        (Some(0), Some(0)) => "Top left",
        (Some(2), Some(0)) => "Top right",
        (Some(0), Some(2)) => "Bottom left",
        (Some(2), Some(2)) => "Bottom right",
        (Some(1), Some(1)) => "Centre",
        (Some(1), Some(0)) => "Top centre",
        (Some(1), Some(2)) => "Bottom centre",
        (Some(0), Some(1)) => "Centre left",
        (Some(2), Some(1)) => "Centre right",
        _ => "",
    };
    if named.is_empty() {
        let pc = |v: f32| (v * 100.0).round() as i32;
        format!("x {}%, y {}%", pc(p[0]), pc(p[1]))
    } else {
        named.to_string()
    }
}

pub fn due(checked: Option<Instant>, now: Instant) -> bool {
    checked.is_none_or(|t| now.saturating_duration_since(t) >= CHECK_EVERY)
}

pub fn changed(seen: Option<SystemTime>, now: Option<SystemTime>) -> bool {
    seen != now
}

pub fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let mut buf = vec![0; reader.output_buffer_size()?];
    reader.next_frame(&mut buf).ok()?;
    let (kind, _) = reader.output_color_type();
    let n = kind.samples();
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in buf.chunks_exact(n).take(w * h) {
        match px {
            [g] => rgba.extend_from_slice(&[*g, *g, *g, 255]),
            [g, a] => rgba.extend_from_slice(&[*g, *g, *g, *a]),
            [r, g, b] => rgba.extend_from_slice(&[*r, *g, *b, 255]),
            [r, g, b, a] => rgba.extend_from_slice(&[*r, *g, *b, *a]),
            _ => return None,
        }
    }
    (rgba.len() == w * h * 4).then(|| ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

#[derive(Clone, Default)]
struct Shown {
    checked: Option<Instant>,
    stamp: Option<SystemTime>,
    tex: Option<TextureHandle>,
}

fn smooth() -> TextureOptions {
    TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear))
}

pub fn texture(ctx: &egui::Context, pic: &Picture) -> Option<TextureHandle> {
    match pic {
        Picture::Made(img) => {
            let id = Id::new(("place-made", Arc::as_ptr(img) as usize));
            if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
                return Some(t);
            }
            let t = ctx.load_texture("place-made", (**img).clone(), smooth());
            ctx.data_mut(|d| d.insert_temp(id, t.clone()));
            Some(t)
        }
        Picture::File(path) => {
            let id = Id::new(("place-file", path));
            let mut s: Shown = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
            let now = Instant::now();
            if due(s.checked, now) {
                s.checked = Some(now);
                let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                if stamp.is_none() {
                    s.stamp = None;
                    s.tex = None;
                } else if changed(s.stamp, stamp) {
                    if let Some(img) = std::fs::read(path).ok().and_then(|b| decode(&b)) {
                        match &mut s.tex {
                            Some(t) => t.set(img, smooth()),
                            None => s.tex = Some(ctx.load_texture("place-file", img, smooth())),
                        }
                        s.stamp = stamp;
                    }
                }
                ctx.data_mut(|d| d.insert_temp(id, s.clone()));
            }
            ctx.request_repaint_after(CHECK_EVERY);
            s.tex
        }
    }
}

pub fn view(
    ui: &mut Ui,
    id: Id,
    title: &str,
    pic: Option<&Picture>,
    value: &mut [f32; 2],
    default: [f32; 2],
) -> bool {
    let tex = pic.and_then(|x| texture(ui.ctx(), x));
    let px = tex
        .as_ref()
        .map_or(BOX, |t| [t.size()[0] as f32, t.size()[1] as f32]);
    let s = share(px);
    let avail = ui.available_width();
    let mut changed = false;
    let gap = 2.0 * ui.spacing().item_spacing.x + 2.0;
    let room = (avail - 18.0 - SIDE - gap).min(WIDTH);
    if room >= 260.0 {
        ui.horizontal_top(|ui| {
            changed |= screen(ui, id, room, title, tex.as_ref(), s, value);
            ui.add_space(18.0);
            ui.vertical(|ui| changed |= controls(ui, s, value, default));
        });
    } else {
        changed |= screen(ui, id, avail.min(WIDTH), title, tex.as_ref(), s, value);
        ui.add_space(12.0);
        changed |= controls(ui, s, value, default);
    }
    changed
}

fn screen(
    ui: &mut Ui,
    id: Id,
    w: f32,
    title: &str,
    tex: Option<&TextureHandle>,
    s: [f32; 2],
    value: &mut [f32; 2],
) -> bool {
    let p = pal_ui(ui);
    let h = w * 9.0 / 16.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let size = vec2(s[0] * w, s[1] * h);
    let tol = [SNAP / w, SNAP / h];
    let start_id = id.with("start");
    let mut guides = [None, None];
    let mut changed = false;
    let mut set = |next: [f32; 2], value: &mut [f32; 2]| {
        if next != *value {
            *value = next;
            changed = true;
        }
    };
    let held = Rect::from_min_size(to_px(rect, clamp(*value, s)), size);
    if resp.dragged() {
        let origin = ui.input(|i| i.pointer.press_origin());
        if let (Some(o), Some(at)) = (origin, resp.interact_pointer_pos()) {
            let start = ui.data_mut(|d| {
                *d.get_temp_mut_or_insert_with(start_id, || {
                    if held.contains(o) {
                        clamp(*value, s)
                    } else {
                        clamp(to_frac(rect, o - size / 2.0), s)
                    }
                })
            });
            let raw = [start[0] + (at.x - o.x) / w, start[1] + (at.y - o.y) / h];
            let (next, g) = snap(clamp(raw, s), s, tol);
            guides = g;
            set(next, value);
        }
    } else {
        ui.data_mut(|d| d.remove::<[f32; 2]>(start_id));
        if resp.clicked() {
            if let Some(o) = resp.interact_pointer_pos().filter(|o| !held.contains(*o)) {
                let next = snap(clamp(to_frac(rect, o - size / 2.0), s), s, tol).0;
                set(next, value);
            }
        }
    }
    let at = Rect::from_min_size(to_px(rect, clamp(*value, s)), size);
    let over = resp.hovered() && resp.hover_pos().is_some_and(|q| at.contains(q));
    if resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if over {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    } else if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let painter = ui.painter_at(rect.expand(4.0));
    let round = Rounding::same(theme::radius::SMALL + 2.0);
    wallpaper(&painter, rect, round, &p);
    taskbar(&painter, rect, &p);
    let line = p.accent.gamma_multiply(0.7);
    if let Some(g) = guides[0] {
        let x = (rect.left() + g * w).clamp(rect.left() + 1.0, rect.right() - 1.0);
        painter.vline(x, rect.y_range(), Stroke::new(1.0_f32, line));
    }
    if let Some(g) = guides[1] {
        let y = (rect.top() + g * h).clamp(rect.top() + 1.0, rect.bottom() - 1.0);
        painter.hline(rect.x_range(), y, Stroke::new(1.0_f32, line));
    }
    match tex {
        Some(t) => {
            painter.image(
                t.id(),
                at,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let r = Rounding::same((at.height() * 0.18).clamp(2.0, 6.0));
            painter.rect(
                at,
                r,
                Color32::from_black_alpha(if p.dark { 150 } else { 110 }),
                Stroke::new(1.0_f32, Color32::from_white_alpha(70)),
            );
            let font = FontId::new((at.height() * 0.36).clamp(8.0, 12.0), theme::semibold());
            painter.text(
                at.center(),
                Align2::CENTER_CENTER,
                title,
                font,
                Color32::from_white_alpha(230),
            );
        }
    }
    if over || resp.dragged() || resp.has_focus() {
        painter.rect_stroke(
            at.expand(2.5),
            Rounding::same(4.0),
            Stroke::new(1.5_f32, p.accent),
        );
    }
    painter.rect_stroke(rect, round, Stroke::new(1.0_f32, p.line));
    changed
}

fn wallpaper(painter: &egui::Painter, rect: Rect, round: Rounding, p: &Palette) {
    let (a, b, c) = if p.dark {
        (
            Color32::from_rgb(0x1d, 0x2b, 0x4f),
            Color32::from_rgb(0x2c, 0x23, 0x52),
            Color32::from_rgb(0x4a, 0x2a, 0x55),
        )
    } else {
        (
            Color32::from_rgb(0xb8, 0xd3, 0xf5),
            Color32::from_rgb(0xcd, 0xcd, 0xf4),
            Color32::from_rgb(0xec, 0xd5, 0xee),
        )
    };
    let tint = |q: Pos2| {
        let t = ((q.x - rect.left()) / rect.width() * 0.65
            + (q.y - rect.top()) / rect.height() * 0.35)
            .clamp(0.0, 1.0);
        if t < 0.5 {
            mix(a, b, t * 2.0)
        } else {
            mix(b, c, t * 2.0 - 1.0)
        }
    };
    let mut path = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(&mut path, rect, round);
    painter.add(fan(rect.center(), &path, tint));
    let glow = pos2(
        rect.left() + rect.width() * 0.7,
        rect.top() + rect.height() * 0.36,
    );
    let r = rect.height() * 0.6;
    let top = if p.dark { 20.0 } else { 26.0 };
    let n = 64u32;
    let rings = [(0.0, 1.0), (0.3, 0.7), (0.6, 0.3), (1.0, 0.0)];
    let mut m = Mesh::default();
    for (k, (f, a)) in rings.into_iter().enumerate() {
        let color = Color32::from_white_alpha((top * a) as u8);
        let count = if k == 0 { 1 } else { n };
        for i in 0..count {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            m.vertices.push(Vertex {
                pos: glow + vec2(t.cos(), t.sin()) * r * f,
                uv: WHITE_UV,
                color,
            });
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.indices.extend_from_slice(&[0, 1 + i, 1 + j]);
        for ring in 0..2 {
            let (a, b) = (1 + ring * n, 1 + (ring + 1) * n);
            m.indices
                .extend_from_slice(&[a + i, b + i, b + j, a + i, b + j, a + j]);
        }
    }
    painter.with_clip_rect(rect.shrink(1.0)).add(m);
}

fn fan(center: Pos2, path: &[Pos2], tint: impl Fn(Pos2) -> Color32) -> Mesh {
    let mut m = Mesh::default();
    m.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color: tint(center),
    });
    for q in path {
        m.vertices.push(Vertex {
            pos: *q,
            uv: WHITE_UV,
            color: tint(*q),
        });
    }
    let n = path.len() as u32;
    for i in 0..n {
        m.indices
            .extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n.max(1)]);
    }
    m
}

fn taskbar(painter: &egui::Painter, rect: Rect, p: &Palette) {
    let tb = (rect.height() * 48.0 / 1080.0).max(7.0);
    let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - tb), rect.max);
    let r = theme::radius::SMALL + 2.0;
    let fill = if p.dark {
        Color32::from_black_alpha(110)
    } else {
        Color32::from_white_alpha(150)
    };
    painter.rect_filled(
        bar,
        Rounding {
            nw: 0.0,
            ne: 0.0,
            sw: r,
            se: r,
        },
        fill,
    );
    let dot = (tb * 0.5).round().max(3.0);
    let gap = dot * 0.7;
    let n = 5;
    let total = n as f32 * dot + (n - 1) as f32 * gap;
    let mut x = bar.center().x - total / 2.0;
    let ink = if p.dark {
        Color32::from_white_alpha(70)
    } else {
        Color32::from_black_alpha(55)
    };
    for i in 0..n {
        let c = if i == 0 { p.accent } else { ink };
        painter.rect_filled(
            Rect::from_min_size(pos2(x, bar.center().y - dot / 2.0), vec2(dot, dot)),
            Rounding::same(1.5),
            c,
        );
        x += dot + gap;
    }
    let right = bar.right() - 10.0;
    for (w, dy) in [(14.0, -1.6), (10.0, 1.6)] {
        painter.hline(
            (right - w)..=right,
            bar.center().y + dy,
            Stroke::new(1.2_f32, ink),
        );
    }
}

fn controls(ui: &mut Ui, s: [f32; 2], value: &mut [f32; 2], default: [f32; 2]) -> bool {
    let p = pal_ui(ui);
    let mut changed = false;
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            egui::RichText::new(caption(clamp(*value, s), s))
                .font(FontId::new(14.0, theme::semibold()))
                .color(p.text),
        );
        ui.label(
            egui::RichText::new("Drag the preview or pick a corner")
                .size(12.0)
                .color(p.text_3),
        );
    });
    ui.add_space(4.0);
    for pair in CORNERS.chunks(2) {
        ui.horizontal(|ui| {
            for (c, label) in pair {
                let spot = corner(*c, s);
                let on = (spot[0] - value[0]).abs() <= EPS && (spot[1] - value[1]).abs() <= EPS;
                if pad(ui, *c, label, on).clicked() && !on {
                    *value = spot;
                    changed = true;
                }
            }
        });
    }
    let home = clamp(default, s);
    let moved = (home[0] - value[0]).abs() > EPS || (home[1] - value[1]).abs() > EPS;
    ui.add_enabled_ui(moved, |ui| {
        if crate::widgets::quiet_button(ui, Some(theme::icon::REFRESH), "Reset to default")
            .clicked()
        {
            *value = home;
            changed = true;
        }
    });
    changed
}

fn pad(ui: &mut Ui, c: Corner, label: &str, on: bool) -> egui::Response {
    let p = pal_ui(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(PAD_W, 30.0), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let lift = if p.dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let base = if on {
        mix(p.card_hi, p.accent, if p.dark { 0.28 } else { 0.16 })
    } else {
        p.card_hi
    };
    let bg = mix(base, lift, 0.08 * hover);
    ui.painter()
        .rect_filled(rect, Rounding::same(theme::radius::CONTROL - 2.0), bg);
    let ink = if on { p.accent } else { p.text_2 };
    let icon = Rect::from_min_size(
        pos2(rect.left() + 11.0, rect.center().y - 5.5),
        vec2(15.0, 11.0),
    );
    ui.painter()
        .rect_stroke(icon, Rounding::same(2.0), Stroke::new(1.2_f32, ink));
    let dot = vec2(6.0, 4.0);
    let inner = icon.shrink(2.0);
    let min = match c {
        Corner::TopLeft => inner.min,
        Corner::TopRight => pos2(inner.right() - dot.x, inner.top()),
        Corner::BottomLeft => pos2(inner.left(), inner.bottom() - dot.y),
        Corner::BottomRight => inner.max - dot,
    };
    ui.painter()
        .rect_filled(Rect::from_min_size(min, dot), Rounding::same(1.0), ink);
    ui.painter().text(
        pos2(icon.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        if on { p.text } else { p.text_2 },
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.on_hover_text(format!("Move it to the {}", label.to_lowercase()))
}

fn paint(img: &mut [u8], w: usize, h: usize, r: Rect, radius: f32, c: [u8; 4]) {
    let (x0, y0) = (
        r.left().floor().max(0.0) as usize,
        r.top().floor().max(0.0) as usize,
    );
    let (x1, y1) = (
        (r.right().ceil() as usize).min(w),
        (r.bottom().ceil() as usize).min(h),
    );
    let half = r.size() / 2.0 - vec2(radius, radius);
    for y in y0..y1 {
        for x in x0..x1 {
            let q = pos2(x as f32 + 0.5, y as f32 + 0.5) - r.center();
            let d = vec2((q.x.abs() - half.x).max(0.0), (q.y.abs() - half.y).max(0.0));
            let inside = (q.x.abs() - half.x).max(q.y.abs() - half.y).min(0.0);
            let dist = d.length() + inside - radius;
            let cover = (0.5 - dist).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let i = (y * w + x) * 4;
            let sa = c[3] as f32 / 255.0 * cover;
            let da = img[i + 3] as f32 / 255.0;
            let oa = sa + da * (1.0 - sa);
            if oa <= 0.0 {
                continue;
            }
            for k in 0..3 {
                let v = (c[k] as f32 * sa + img[i + k] as f32 * da * (1.0 - sa)) / oa;
                img[i + k] = v.round() as u8;
            }
            img[i + 3] = (oa * 255.0).round() as u8;
        }
    }
}

pub fn keyboard() -> ColorImage {
    let (u, g, pad) = (56.0_f32, 8.0_f32, 18.0_f32);
    let rows: [(f32, &[(f32, bool)]); 4] = [
        (
            0.5,
            &[(1.0, false), (1.0, true), (1.0, false), (1.0, false)],
        ),
        (
            0.75,
            &[(1.0, true), (1.0, false), (1.0, false), (1.0, false)],
        ),
        (
            0.0,
            &[(1.75, true), (1.0, false), (1.0, false), (1.0, false)],
        ),
        (0.0, &[(1.25, false), (3.5, false)]),
    ];
    let keys_w = 290.0;
    let mouse = vec2(76.0, 112.0);
    let w = (pad * 3.0 + keys_w + mouse.x) as usize;
    let h = (pad * 2.0 + 4.0 * u + 3.0 * g) as usize;
    let mut img = vec![0u8; w * h * 4];
    let full = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
    paint(&mut img, w, h, full, 22.0, [14, 16, 22, 215]);
    let down = [10, 132, 255, 255];
    let up = [58, 62, 74, 255];
    let top = [255, 255, 255, 22];
    for (row, (off, keys)) in rows.iter().enumerate() {
        let mut x = pad + off * u;
        let y = pad + row as f32 * (u + g);
        for (span, pressed) in keys.iter() {
            let kw = span * u;
            let r = Rect::from_min_size(pos2(x, y), vec2(kw, u));
            paint(&mut img, w, h, r, 9.0, if *pressed { down } else { up });
            let shine = Rect::from_min_size(r.min + vec2(3.0, 3.0), vec2(kw - 6.0, u * 0.42));
            paint(&mut img, w, h, shine, 7.0, top);
            x += kw + g;
        }
    }
    let m = Rect::from_min_size(pos2(pad * 2.0 + keys_w, (h as f32 - mouse.y) / 2.0), mouse);
    paint(&mut img, w, h, m, 34.0, up);
    let left = Rect::from_min_max(
        m.min + vec2(4.0, 4.0),
        pos2(m.center().x - 2.0, m.top() + 46.0),
    );
    paint(&mut img, w, h, left, 14.0, down);
    let wheel = Rect::from_center_size(pos2(m.center().x, m.top() + 26.0), vec2(8.0, 18.0));
    paint(&mut img, w, h, wheel, 4.0, [200, 205, 215, 255]);
    ColorImage::from_rgba_unmultiplied([w, h], &img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Harness;

    fn rect() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(360.0, 202.5))
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn pixels_and_fractions_round_trip() {
        let r = rect();
        for p in [[0.0, 0.0], [0.25, 0.75], [1.0, 1.0], [0.123, 0.456]] {
            assert!(near(to_frac(r, to_px(r, p)), p), "{p:?}");
        }
        assert_eq!(to_px(r, [0.5, 0.5]), r.center());
        assert_eq!(to_px(r, [0.0, 0.0]), r.min);
    }

    #[test]
    fn the_preview_stays_inside_the_screen() {
        let s = share([480.0, 270.0]);
        assert!(near(s, [0.25, 0.25]));
        assert!(near(clamp([0.9, -0.2], s), [0.75, 0.0]));
        assert!(near(clamp([-1.0, 2.0], s), [0.0, 0.75]));
        assert!(near(clamp([0.4, 0.4], s), [0.4, 0.4]));
        assert!(near(clamp([0.5, 0.5], [1.0, 1.0]), [0.0, 0.0]));
        assert!(near(share([4000.0, 50.0]), [1.0, 50.0 / 1080.0]));
    }

    #[test]
    fn it_snaps_to_edges_corners_and_the_centre() {
        let s = [0.2, 0.1];
        let tol = [0.02, 0.03];
        let (p, g) = snap([0.79, 0.885], s, tol);
        assert!(near(p, [0.8, 0.9]));
        assert_eq!(g, [Some(1.0), Some(1.0)]);
        let (p, g) = snap([0.01, 0.02], s, tol);
        assert!(near(p, [0.0, 0.0]));
        assert_eq!(g, [Some(0.0), Some(0.0)]);
        let (p, g) = snap([0.41, 0.44], s, tol);
        assert!(near(p, [0.4, 0.45]));
        assert_eq!(g, [Some(0.5), Some(0.5)]);
        let (p, g) = snap([0.3, 0.6], s, tol);
        assert!(near(p, [0.3, 0.6]));
        assert_eq!(g, [None, None]);
    }

    #[test]
    fn corners_and_captions_line_up() {
        let s = [0.2, 0.1];
        assert!(near(corner(Corner::TopLeft, s), [0.0, 0.0]));
        assert!(near(corner(Corner::TopRight, s), [0.8, 0.0]));
        assert!(near(corner(Corner::BottomLeft, s), [0.0, 0.9]));
        assert!(near(corner(Corner::BottomRight, s), [0.8, 0.9]));
        for (c, label) in CORNERS {
            assert_eq!(caption(corner(c, s), s), label);
        }
        assert_eq!(caption([0.4, 0.45], s), "Centre");
        assert_eq!(caption([0.4, 0.0], s), "Top centre");
        assert_eq!(caption([0.12, 0.7], s), "x 12%, y 70%");
    }

    #[test]
    fn the_file_is_read_again_only_when_it_changes() {
        let t = Instant::now();
        assert!(due(None, t));
        assert!(!due(Some(t), t + Duration::from_millis(200)));
        assert!(due(Some(t), t + CHECK_EVERY));
        let a = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let b = a + Duration::from_millis(1);
        assert!(!changed(Some(a), Some(a)));
        assert!(changed(Some(a), Some(b)));
        assert!(changed(None, Some(a)));
        assert!(changed(Some(a), None));
        assert!(!changed(None, None));
    }

    #[test]
    fn only_a_plain_png_name_is_used() {
        let dir = Path::new("addon");
        assert_eq!(file_of(dir, "preview.png"), Some(dir.join("preview.png")));
        assert_eq!(file_of(dir, "..\\x.png"), None);
        assert_eq!(file_of(dir, "a/b.png"), None);
        assert_eq!(file_of(dir, "c:x.png"), None);
        assert_eq!(file_of(dir, "x.exe"), None);
        assert_eq!(file_of(dir, ""), None);
    }

    #[test]
    fn a_png_decodes_at_full_size() {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, 300, 100);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&vec![200; 300 * 100 * 4]).unwrap();
        }
        let img = decode(&bytes).unwrap();
        assert_eq!(img.size, [300, 100]);
        assert!(decode(b"not a png").is_none());
        let k = keyboard();
        assert!(k.size[0] > 300 && k.size[1] > 200);
        assert!(k.pixels.iter().any(|c| c.a() > 200));
    }

    fn draw(h: &mut Harness, pic: Option<&Picture>, v: &mut [f32; 2], changed: &mut bool) {
        h.frame(Vec::new(), egui::Modifiers::NONE, &mut |ui| {
            *changed |= view(ui, Id::new("t"), "Key overlay", pic, v, [0.0, 0.0]);
        });
    }

    #[test]
    fn a_missing_png_shows_a_box_with_the_title() {
        let mut h = Harness::new();
        let pic = Picture::File(PathBuf::from("no-such-folder-for-place\\preview.png"));
        let (mut v, mut c) = ([0.3, 0.3], false);
        draw(&mut h, Some(&pic), &mut v, &mut c);
        assert!(h.has("Key overlay"));
        assert!(h.has("x 30%, y 30%"));
        let made = Picture::Made(Arc::new(keyboard()));
        draw(&mut h, Some(&made), &mut v, &mut c);
        assert!(!h.has("Key overlay"));
        assert!(!c);
    }

    #[test]
    fn the_corner_buttons_and_reset_set_the_value() {
        let mut h = Harness::new();
        let (mut v, mut c) = ([0.3, 0.3], false);
        draw(&mut h, None, &mut v, &mut c);
        let s = share(BOX);
        for (corner_at, label) in CORNERS {
            h.click_text(label, &mut |ui| {
                c |= view(ui, Id::new("t"), "Key overlay", None, &mut v, [0.0, 0.0]);
            });
            assert!(near(v, corner(corner_at, s)), "{label}");
            assert!(h.at(label, 1).is_some(), "{label} shows as the caption");
        }
        assert!(c);
        h.click_text("Reset to default", &mut |ui| {
            view(ui, Id::new("t"), "Key overlay", None, &mut v, [0.0, 0.0]);
        });
        assert!(near(v, [0.0, 0.0]));
    }

    #[test]
    fn dragging_moves_and_snaps_the_preview() {
        let mut h = Harness::new();
        let (mut v, mut c) = ([0.3, 0.3], false);
        draw(&mut h, None, &mut v, &mut c);
        let at = h.at("Key overlay", 0).unwrap().center();
        let mut go = |h: &mut Harness, from: Pos2, to: Pos2, v: &mut [f32; 2]| {
            h.drag(from, to, &mut |ui| {
                c |= view(ui, Id::new("t"), "Key overlay", None, v, [0.0, 0.0]);
            });
        };
        go(&mut h, at, at + vec2(36.0, 20.25), &mut v);
        assert!(near(v, [0.4, 0.4]), "{v:?}");
        let at = h.at("Key overlay", 0).unwrap().center();
        go(&mut h, at, at + vec2(-500.0, 900.0), &mut v);
        assert!(near(v, [0.0, 1.0 - BOX[1] / SCREEN[1]]), "{v:?}");
        assert!(h.at("Bottom left", 1).is_some());
        let at = h.at("Key overlay", 0).unwrap().center();
        let s = share(BOX);
        let centre = (1.0 - s[0]) / 2.0 * WIDTH;
        go(&mut h, at, at + vec2(centre + 3.0, 0.0), &mut v);
        assert!(near(v, [(1.0 - s[0]) / 2.0, 1.0 - s[1]]), "{v:?}");
        assert!(h.has("Bottom centre"));
        assert!(c);
    }
}
