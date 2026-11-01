use crate::layout::{Board, Cap, Layout, DOWN, LEFT, RIGHT, UP};
use crate::look::{mix, Look, Paint, Theme};
use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use std::path::PathBuf;

const FONTS: &[&str] = &["segoeuib.ttf", "segoeui.ttf", "arialbd.ttf", "arial.ttf"];

pub fn font() -> Option<FontVec> {
    let dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    FONTS
        .iter()
        .find_map(|n| FontVec::try_from_vec(std::fs::read(dir.join(n)).ok()?).ok())
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lit {
    pub keys: Vec<f32>,
    pub mouse: [f32; 3],
    pub scroll: f32,
    pub up: bool,
}

impl Lit {
    pub fn idle(b: &Board) -> Self {
        Lit {
            keys: vec![0.0; b.caps.len()],
            ..Default::default()
        }
    }

    pub fn demo(b: &Board, vks: &[u8]) -> Self {
        Lit {
            keys: b
                .caps
                .iter()
                .map(|c| {
                    if c.vks.iter().any(|v| vks.contains(v)) {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect(),
            mouse: [1.0, 0.0, 0.0],
            scroll: 0.0,
            up: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub unit: f32,
    pub pad: i32,
    pub gap: i32,
    pub w: i32,
    pub h: i32,
}

impl Frame {
    pub fn new(b: &Board, unit: f32) -> Self {
        let unit = unit.max(8.0);
        let pad = (unit * 0.3).round() as i32;
        let gap = ((unit * 0.08).round() as i32).max(2);
        Frame {
            unit,
            pad,
            gap,
            w: pad * 2 + (b.w * unit).round() as i32 - gap,
            h: pad * 2 + (b.h * unit).round() as i32 - gap,
        }
    }

    pub fn boxed(&self, x: f32, y: f32, w: f32, h: f32) -> [i32; 4] {
        let at = |v: f32| self.pad + (v * self.unit).round() as i32;
        [at(x), at(y), at(x + w) - self.gap, at(y + h) - self.gap]
    }

    pub fn cap(&self, c: &Cap) -> [i32; 4] {
        self.boxed(c.x, c.y, c.w, c.h)
    }
}

pub fn unit_for(layout: Layout, size: u32, dpi: f32) -> f32 {
    layout.unit() * size as f32 / 100.0 * dpi
}

pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<[f32; 4]>,
}

impl Image {
    fn new(w: i32, h: i32) -> Self {
        Image {
            w,
            h,
            px: vec![[0.0; 4]; (w.max(0) * h.max(0)) as usize],
        }
    }

    fn put(&mut self, x: i32, y: i32, rgb: [u8; 3], a: f32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || a <= 0.0 {
            return;
        }
        let a = a.min(1.0);
        let d = &mut self.px[(y * self.w + x) as usize];
        let k = 1.0 - a;
        for i in 0..3 {
            d[i] = rgb[i] as f32 / 255.0 * a + d[i] * k;
        }
        d[3] = a + d[3] * k;
    }

    fn paint(&mut self, bx: [f32; 4], rgb: [u8; 3], alpha: impl Fn(f32, f32) -> f32) {
        let x0 = bx[0].floor().max(0.0) as i32;
        let y0 = bx[1].floor().max(0.0) as i32;
        let x1 = (bx[2].ceil() as i32).min(self.w);
        let y1 = (bx[3].ceil() as i32).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let a = alpha(x as f32 + 0.5, y as f32 + 0.5);
                self.put(x, y, rgb, a);
            }
        }
    }

    fn fill(&mut self, bx: [f32; 4], c: Paint, sd: impl Fn(f32, f32) -> f32) {
        self.paint(bx, c.rgb, |x, y| cover(sd(x, y)) * c.a);
    }

    pub fn fade(&mut self, k: f32) {
        for p in &mut self.px {
            for v in p.iter_mut() {
                *v *= k;
            }
        }
    }

    pub fn bgra(&self) -> Vec<u8> {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.px
            .iter()
            .flat_map(|p| [q(p[2]), q(p[1]), q(p[0]), q(p[3])])
            .collect()
    }

    pub fn rgba(&self) -> Vec<u8> {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.px
            .iter()
            .flat_map(|p| {
                if p[3] <= 0.0 {
                    return [0, 0, 0, 0];
                }
                let s = |v: f32| q(v / p[3]);
                [s(p[0]), s(p[1]), s(p[2]), q(p[3])]
            })
            .collect()
    }
}

fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

fn rounded(px: f32, py: f32, r: [f32; 4], rad: f32) -> f32 {
    let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
    let (hx, hy) = ((r[2] - r[0]) / 2.0, (r[3] - r[1]) / 2.0);
    let rad = rad.min(hx).min(hy).max(0.0);
    let qx = (px - cx).abs() - (hx - rad);
    let qy = (py - cy).abs() - (hy - rad);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad
}

fn grow(r: [f32; 4], by: f32) -> [f32; 4] {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}

fn poly(px: f32, py: f32, pts: &[(f32, f32)]) -> f32 {
    let n = pts.len();
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum();
    let turn = if area < 0.0 { -1.0 } else { 1.0 };
    let mut d = f32::MIN;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let len = ex.hypot(ey).max(1e-6) * turn;
        let (nx, ny) = (ey / len, -ex / len);
        d = d.max((px - a.0) * nx + (py - a.1) * ny);
    }
    d
}

fn arrow(img: &mut Image, label: &str, cx: f32, cy: f32, s: f32, c: Paint) {
    let turn = |x: f32, y: f32| -> (f32, f32) {
        let (dx, dy) = (x - cx, y - cy);
        match label {
            DOWN => (dx, -dy),
            LEFT => (dy, dx),
            RIGHT => (dy, -dx),
            _ => (dx, dy),
        }
    };
    let head = [
        (0.0, -0.5 * s),
        (0.42 * s, -0.02 * s),
        (-0.42 * s, -0.02 * s),
    ];
    let stem = [-0.09 * s, -0.1 * s, 0.09 * s, 0.48 * s];
    let bx = [cx - s, cy - s, cx + s, cy + s];
    img.fill(bx, c, |x, y| {
        let (lx, ly) = turn(x, y);
        poly(lx, ly, &head).min(rounded(lx, ly, stem, 0.04 * s))
    });
}

fn text(img: &mut Image, f: &FontVec, label: &str, size: f32, cx: f32, cy: f32, c: Paint) {
    let sf = f.as_scaled(PxScale::from(size));
    let cap = f
        .outline_glyph(sf.scaled_glyph('H'))
        .map(|g| -g.px_bounds().min.y)
        .unwrap_or(size * 0.7);
    let width = advance(f, label, size);
    let mut x = (cx - width / 2.0).round();
    let base = (cy + cap / 2.0).round();
    let mut prev = None;
    for ch in label.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(PxScale::from(size), point(x, base));
        if let Some(o) = f.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, v| {
                img.put(
                    b.min.x as i32 + gx as i32,
                    b.min.y as i32 + gy as i32,
                    c.rgb,
                    v.powf(0.85) * c.a,
                );
            });
        }
        x += sf.h_advance(id);
        prev = Some(id);
    }
}

fn advance(f: &FontVec, label: &str, size: f32) -> f32 {
    let sf = f.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for ch in label.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            w += sf.kern(p, id);
        }
        w += sf.h_advance(id);
        prev = Some(id);
    }
    w
}

fn label_size(f: &FontVec, label: &str, u: f32, room: f32) -> f32 {
    if label.chars().count() == 1 {
        return (u * 0.42).max(10.0);
    }
    let mut size = (u * 0.3).max(9.0);
    while size > 6.0 && advance(f, label, size) > room {
        size -= 0.5;
    }
    size
}

fn glow(img: &mut Image, r: [f32; 4], look: &Look, t: f32, u: f32, sd: impl Fn(f32, f32) -> f32) {
    if t <= 0.0 || look.glow.a <= 0.0 {
        return;
    }
    let sigma = look.glow_size * u;
    let a = look.glow.a * t;
    img.paint(grow(r, sigma * 3.0), look.glow.rgb, |x, y| {
        let d = sd(x, y).max(0.0) / sigma;
        a * (-d * d).exp()
    });
}

fn key(img: &mut Image, fr: &Frame, f: Option<&FontVec>, c: &Cap, look: &Look, t: f32) {
    let u = fr.unit;
    let b = fr.cap(c);
    let r = [b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32];
    let rad = (u * 0.14).max(2.0);
    let lip = ((u * 0.09).round()).max(2.0);
    let face = [r[0], r[1] + t, r[2], r[3] - lip * (1.0 - t)];
    let low = [r[0], r[1] + lip, r[2], r[3]];
    img.fill(low, mix(look.lip, look.hot_lip, t), |x, y| {
        rounded(x, y, r, rad).max(-rounded(x, y, face, rad) - 0.5)
    });
    img.fill(face, mix(look.cap, look.hot, t), |x, y| {
        rounded(x, y, face, rad)
    });
    let edge = mix(look.edge, Paint { a: 0.0, ..look.hot }, t);
    let bw = (u * 0.035).max(1.0);
    img.fill(face, edge, |x, y| {
        let d = rounded(x, y, face, rad);
        d.max(-d - bw)
    });
    let ink = mix(look.label, look.hot_label, t);
    let (cx, cy) = ((face[0] + face[2]) / 2.0, (face[1] + face[3]) / 2.0);
    if [UP, DOWN, LEFT, RIGHT].contains(&c.label) {
        arrow(img, c.label, cx, cy, u * 0.36, ink);
    } else if let Some(f) = f {
        let room = face[2] - face[0] - u * 0.24;
        text(
            img,
            f,
            c.label,
            label_size(f, c.label, u, room),
            cx,
            cy,
            ink,
        );
    }
}

fn mouse(img: &mut Image, fr: &Frame, b: &Board, look: &Look, lit: &Lit) {
    let Some(m) = b.mouse else { return };
    let u = fr.unit;
    let bx = fr.boxed(m.x, m.y, m.w, m.h);
    let body = [bx[0] as f32, bx[1] as f32, bx[2] as f32, bx[3] as f32];
    let (w, h) = (body[2] - body[0], body[3] - body[1]);
    let rb = w.min(h) * 0.46;
    let g = fr.gap as f32;
    let cx = ((body[0] + body[2]) / 2.0).round();
    let split = (body[1] + h * 0.44).round();
    let small = (u * 0.08).max(1.5);
    let in_body =
        |x: f32, y: f32, part: [f32; 4]| rounded(x, y, body, rb).max(rounded(x, y, part, small));
    let parts = [
        ([body[0], body[1], cx - g / 2.0, split], lit.mouse[0]),
        ([cx + g / 2.0, body[1], body[2], split], lit.mouse[2]),
    ];
    let ww = (w * 0.13).max(4.0).round();
    let top = body[1] + h * 0.1;
    let bottom = split - h * 0.06;
    let wheel = [cx - ww / 2.0, top, cx + ww / 2.0, bottom];
    let spin = lit.mouse[1].max(lit.scroll);
    for (part, t) in parts {
        glow(img, part, look, t * 0.8, u, |x, y| in_body(x, y, part));
    }
    glow(img, wheel, look, spin, u, |x, y| {
        rounded(x, y, wheel, ww / 2.0)
    });
    let bw = (u * 0.035).max(1.0);
    for (part, t) in parts {
        img.fill(part, mix(look.cap, look.hot, t), |x, y| in_body(x, y, part));
        let edge = mix(look.edge, Paint { a: 0.0, ..look.hot }, t);
        img.fill(part, edge, |x, y| {
            let d = in_body(x, y, part);
            d.max(-d - bw)
        });
    }
    let palm = [body[0], split + g, body[2], body[3]];
    img.fill(palm, mix(look.lip, look.cap, 0.55), |x, y| {
        in_body(x, y, palm)
    });
    img.fill(palm, look.edge, |x, y| {
        let d = in_body(x, y, palm);
        d.max(-d - bw)
    });
    let slot = [cx - ww / 2.0 - g, top - g, cx + ww / 2.0 + g, bottom + g];
    img.fill(slot, look.lip, |x, y| rounded(x, y, slot, ww));
    let idle = mix(look.lip, look.label, 0.45);
    img.fill(wheel, mix(idle, look.hot, spin), |x, y| {
        rounded(x, y, wheel, ww / 2.0)
    });
    if lit.scroll > 0.0 {
        let s = u * 0.3;
        let (ax, ay) = (cx, split + g + (body[3] - split) * 0.32);
        let dir = if lit.up { -1.0 } else { 1.0 };
        let tri = [
            (ax, ay + dir * s * 0.55),
            (ax + s * 0.7, ay - dir * s * 0.35),
            (ax - s * 0.7, ay - dir * s * 0.35),
        ];
        let c = Paint {
            a: look.hot.a * lit.scroll,
            ..look.hot
        };
        img.fill([ax - s, ay - s, ax + s, ay + s], c, |x, y| poly(x, y, &tri));
    }
}

pub fn render(
    b: &Board,
    theme: Theme,
    unit: f32,
    opacity: f32,
    f: Option<&FontVec>,
    lit: &Lit,
) -> Image {
    let fr = Frame::new(b, unit);
    let look = theme.look();
    let mut img = Image::new(fr.w, fr.h);
    let all = [0.0, 0.0, fr.w as f32, fr.h as f32];
    img.fill(all, look.plate, |x, y| rounded(x, y, all, fr.unit * 0.3));
    for (i, c) in b.caps.iter().enumerate() {
        let t = lit.keys.get(i).copied().unwrap_or(0.0);
        if t > 0.0 {
            let k = fr.cap(c);
            let r = [k[0] as f32, k[1] as f32, k[2] as f32, k[3] as f32];
            let rad = fr.unit * 0.14;
            glow(&mut img, r, &look, t, fr.unit, |x, y| rounded(x, y, r, rad));
        }
    }
    for (i, c) in b.caps.iter().enumerate() {
        let t = lit.keys.get(i).copied().unwrap_or(0.0);
        key(&mut img, &fr, f, c, &look, t);
    }
    mouse(&mut img, &fr, b, &look, lit);
    img.fade(opacity.clamp(0.0, 1.0));
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_stay_inside_the_frame_with_gaps() {
        for l in Layout::ALL {
            let b = Board::new(l, true);
            for unit in [16.0, l.unit(), 120.0] {
                let fr = Frame::new(&b, unit);
                for c in &b.caps {
                    let r = fr.cap(c);
                    assert!(r[0] >= fr.pad && r[1] >= fr.pad);
                    assert!(r[2] <= fr.w - fr.pad && r[3] <= fr.h - fr.pad);
                    assert!(r[2] - r[0] > fr.gap && r[3] - r[1] > fr.gap);
                }
            }
        }
    }

    #[test]
    fn wasd_is_about_a_fifth_of_a_1080p_screen() {
        let b = Board::new(Layout::Wasd, true);
        let fr = Frame::new(&b, unit_for(Layout::Wasd, 100, 1.0));
        let share = fr.w as f32 / 1920.0;
        assert!((0.2..0.24).contains(&share), "{share}");
        let big = Frame::new(&b, unit_for(Layout::Wasd, 100, 1.5));
        assert!((big.w as f32 / fr.w as f32 - 1.5).abs() < 0.05);
    }

    #[test]
    fn a_scroll_flash_points_the_right_way() {
        let b = Board::new(Layout::Wasd, true);
        let fr = Frame::new(&b, 58.0);
        let m = b.mouse.unwrap();
        let bx = fr.boxed(m.x, m.y, m.w, m.h);
        let idle = render(&b, Theme::Dark, 58.0, 1.0, None, &Lit::idle(&b));
        let heavy_below = |up: bool| {
            let lit = Lit {
                scroll: 1.0,
                up,
                ..Lit::idle(&b)
            };
            let img = render(&b, Theme::Dark, 58.0, 1.0, None, &lit);
            let mut ys = Vec::new();
            for y in (bx[1] + bx[3]) / 2..bx[3] {
                for x in bx[0]..bx[2] {
                    let i = (y * img.w + x) as usize;
                    if (img.px[i][2] - idle.px[i][2]).abs() > 0.2 {
                        ys.push(y as f32);
                    }
                }
            }
            assert!(ys.len() > 20);
            let lo = ys.iter().copied().fold(f32::MAX, f32::min);
            let hi = ys.iter().copied().fold(f32::MIN, f32::max);
            let mid = (lo + hi) / 2.0;
            ys.iter().filter(|&&y| y >= mid).count() > ys.iter().filter(|&&y| y < mid).count()
        };
        assert!(heavy_below(true));
        assert!(!heavy_below(false));
    }

    #[test]
    fn pressed_keys_render_brighter_and_opacity_scales_alpha() {
        let b = Board::new(Layout::Wasd, true);
        let fr = Frame::new(&b, 55.0);
        let lit = Lit::demo(&b, b"W");
        let img = render(&b, Theme::Dark, 55.0, 1.0, None, &lit);
        assert_eq!((img.w, img.h), (fr.w, fr.h));
        let at = |img: &Image, c: &Cap| {
            let r = fr.cap(c);
            let p = img.px[((r[1] + 8) * img.w + r[0] + 8) as usize];
            p[0].max(p[1]).max(p[2])
        };
        let w = b.caps.iter().find(|c| c.label == "W").unwrap();
        let q = b.caps.iter().find(|c| c.label == "Q").unwrap();
        assert!(at(&img, w) > at(&img, q) + 0.3);
        let half = render(&b, Theme::Dark, 55.0, 0.5, None, &lit);
        let i = (fr.h / 2 * fr.w + fr.w / 2) as usize;
        assert!((half.px[i][3] - img.px[i][3] * 0.5).abs() < 1e-3);
    }
}
