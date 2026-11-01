#![cfg_attr(windows, windows_subsystem = "windows")]

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use sc_core::edit::{Overlay, Plan};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const FONTS: &[&str] = &["impact.ttf", "arialbd.ttf"];
const MAX_LINES: usize = 2;
const MARGIN: f32 = 0.03;
const MIN_PX: f32 = 12.0;
const EMPTY: &str = "Type some top or bottom text first.";

fn fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}

fn load(path: &Path) -> Option<FontVec> {
    FontVec::try_from_vec(std::fs::read(path).ok()?).ok()
}

fn font() -> Option<FontVec> {
    let dir = fonts_dir();
    FONTS.iter().find_map(|n| load(&dir.join(n))).or_else(|| {
        let mut all: Vec<PathBuf> = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("ttf"))
            })
            .collect();
        all.sort();
        all.iter().find_map(|p| load(p))
    })
}

fn stroke(size: f32) -> f32 {
    (size / 12.0).max(2.0)
}

fn advance(font: &FontVec, text: &str, size: f32) -> f32 {
    let f = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            w += f.kern(p, id);
        }
        w += f.h_advance(id);
        prev = Some(id);
    }
    w
}

fn wrap(text: &str, max: f32, width: &impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if width(&format!("{line} {word}")) <= max => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

fn fit(text: &str, w: f32, h: f32, measure: impl Fn(&str, f32) -> f32) -> (f32, Vec<String>) {
    let max = w * (1.0 - 2.0 * MARGIN);
    let small = (h / 24.0).max(MIN_PX);
    let mut size = (h / 9.0).max(small);
    loop {
        let width = |s: &str| measure(s, size) + 2.0 * stroke(size);
        let lines = wrap(text, max, &width);
        let fits = lines.len() <= MAX_LINES && lines.iter().all(|l| width(l) <= max);
        if fits || size <= small {
            return (size, lines);
        }
        size = (size * 0.94).max(small);
    }
}

struct Mask {
    w: usize,
    h: usize,
    cov: Vec<f32>,
}

impl Mask {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            cov: vec![0.0; w * h],
        }
    }

    fn at(&self, x: isize, y: isize) -> f32 {
        if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
            return 0.0;
        }
        self.cov[y as usize * self.w + x as usize]
    }

    fn put(&mut self, x: isize, y: isize, v: f32) {
        if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
            return;
        }
        let c = &mut self.cov[y as usize * self.w + x as usize];
        *c = c.max(v);
    }

    fn grown(&self, r: f32) -> Mask {
        let reach = r.ceil() as isize + 1;
        let mut disk = Vec::new();
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                let a = (r + 0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 {
                    disk.push((dx, dy, a));
                }
            }
        }
        let mut out = Mask::new(self.w, self.h);
        for y in 0..self.h as isize {
            for x in 0..self.w as isize {
                let c = self.at(x, y);
                if c <= 0.0 {
                    continue;
                }
                let edge = c < 1.0
                    || self.at(x - 1, y) < 1.0
                    || self.at(x + 1, y) < 1.0
                    || self.at(x, y - 1) < 1.0
                    || self.at(x, y + 1) < 1.0;
                if !edge {
                    out.put(x, y, 1.0);
                    continue;
                }
                for (dx, dy, a) in &disk {
                    out.put(x + dx, y + dy, a * (c * 2.0).min(1.0));
                }
            }
        }
        out
    }
}

fn line(font: &FontVec, mask: &mut Mask, text: &str, size: f32, x: f32, base: f32) {
    let f = font.as_scaled(PxScale::from(size));
    let mut pen = x;
    let mut prev = None;
    for c in text.chars() {
        let id = f.glyph_id(c);
        if let Some(p) = prev {
            pen += f.kern(p, id);
        }
        let g = id.with_scale_and_position(size, point(pen, base));
        pen += f.h_advance(id);
        prev = Some(id);
        let Some(o) = font.outline_glyph(g) else {
            continue;
        };
        let b = o.px_bounds();
        o.draw(|gx, gy, v| {
            mask.put(
                b.min.x as isize + gx as isize,
                b.min.y as isize + gy as isize,
                v,
            )
        });
    }
}

fn cap(font: &FontVec, size: f32) -> f32 {
    let g = font.glyph_id('H').with_scale(size);
    font.outline_glyph(g)
        .map(|o| o.px_bounds().height())
        .unwrap_or(size * 0.75)
}

fn block(font: &FontVec, mask: &mut Mask, text: &str, top: bool) -> f32 {
    let (w, h) = (mask.w as f32, mask.h as f32);
    let text = text.trim().to_uppercase();
    if text.is_empty() {
        return 0.0;
    }
    let (size, lines) = fit(&text, w, h, |s, px| advance(font, s, px));
    let cap = cap(font, size);
    let pitch = cap * 1.32;
    let edge = h * MARGIN + stroke(size);
    let n = lines.len() as f32;
    let first = if top {
        edge + cap
    } else {
        h - edge - (n - 1.0) * pitch
    };
    for (i, l) in lines.iter().enumerate() {
        let x = (w - advance(font, l, size)) / 2.0;
        line(font, mask, l, size, x, first + i as f32 * pitch);
    }
    size
}

fn caption(font: &FontVec, w: u32, h: u32, top: &str, bottom: &str) -> Overlay {
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    for (text, top) in [(top, true), (bottom, false)] {
        let mut fill = Mask::new(w as usize, h as usize);
        let size = block(font, &mut fill, text, top);
        if size <= 0.0 {
            continue;
        }
        let ring = fill.grown(stroke(size));
        for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (f, o) = (fill.cov[i], ring.cov[i]);
            let a = f + o * (1.0 - f);
            if a <= 0.0 {
                continue;
            }
            let v = (255.0 * f / a).round() as u8;
            let a = (a * 255.0).round() as u8;
            if a >= px[3] {
                *px = [v, v, v, a];
            }
        }
    }
    Overlay {
        width: w,
        height: h,
        rgba,
    }
}

fn target(clip: &Path) -> PathBuf {
    let stem = clip
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".into());
    let dir = clip.parent().unwrap_or(Path::new("."));
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem}-meme.mp4")
        } else {
            format!("{stem}-meme-{n}.mp4")
        };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

fn plain(e: sc_ffmpeg::Error) -> String {
    let e = e.to_string();
    e.trim_start_matches("ffmpeg: ").to_string()
}

fn make(clip: &Path, top: &str, bottom: &str) -> Result<PathBuf, String> {
    if top.trim().is_empty() && bottom.trim().is_empty() {
        return Err(EMPTY.into());
    }
    if !clip.is_file() {
        return Err("That clip could not be found.".into());
    }
    let font = font().ok_or("No usable font was found in the Windows Fonts folder.")?;
    let info = sc_ffmpeg::media_info(clip).map_err(plain)?;
    let mut plan = Plan::whole(info.duration);
    let (w, h) = plan.output_size(info.width, info.height);
    if w < 2 || h < 2 {
        return Err("That clip has no picture.".into());
    }
    plan.overlay = Some(Arc::new(caption(&font, w, h, top, bottom)));
    let dst = target(clip);
    sc_ffmpeg::render_edit(clip, &dst, &plan, &sc_ffmpeg::EditProgress::default())
        .map_err(|e| format!("Could not make the meme: {}", plain(e)))?;
    Ok(std::path::absolute(&dst).unwrap_or(dst))
}

fn reply(done: Result<PathBuf, String>) -> String {
    match done {
        Ok(p) => json!({ "open": p.to_string_lossy() }),
        Err(e) => json!({ "error": e }),
    }
    .to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let value = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    if has("--version") {
        println!("{VERSION}");
        return;
    }
    if has("--uninstall") {
        return;
    }
    if value("--button").as_deref() != Some("caption") {
        return;
    }
    let set = |key: &str| {
        args.windows(2)
            .filter(|w| w[0] == "--set")
            .filter_map(|w| w[1].split_once('='))
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
            .next_back()
            .unwrap_or_default()
    };
    let done = match value("--clip") {
        Some(c) => make(Path::new(&c), &set("top"), &set("bottom")),
        None => Err("No clip was given.".into()),
    };
    println!("{}", reply(done));
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::store::Manifest;
    use serde_json::Value;

    fn chars(s: &str, px: f32) -> f32 {
        s.chars().count() as f32 * px * 0.5
    }

    #[test]
    fn words_wrap_onto_lines_that_fit() {
        let width = |s: &str| s.chars().count() as f32;
        assert_eq!(
            wrap("WHEN THE ENEMY", 9.0, &width),
            vec!["WHEN THE", "ENEMY"]
        );
        assert_eq!(wrap("  A  B ", 9.0, &width), vec!["A B"]);
        assert_eq!(
            wrap("ENORMOUSWORD X", 4.0, &width),
            vec!["ENORMOUSWORD", "X"]
        );
        assert!(wrap("   ", 9.0, &width).is_empty());
    }

    #[test]
    fn text_shrinks_until_it_fits_two_lines() {
        let (big, lines) = fit("HI", 1920.0, 1080.0, chars);
        assert_eq!(big, 120.0);
        assert_eq!(lines, vec!["HI"]);
        let long = "WHEN THE ENEMY PEEKS MID AND YOU ARE STILL RELOADING YOUR GUN";
        let (size, lines) = fit(long, 1920.0, 1080.0, chars);
        assert!(size < big && size >= 45.0, "{size}");
        assert!(lines.len() <= MAX_LINES);
        let max = 1920.0 * (1.0 - 2.0 * MARGIN);
        assert!(lines
            .iter()
            .all(|l| chars(l, size) + 2.0 * stroke(size) <= max));
        let huge = "WORD ".repeat(80);
        let (size, lines) = fit(&huge, 320.0, 180.0, chars);
        assert_eq!(size, MIN_PX);
        assert!(lines.len() > MAX_LINES);
    }

    #[test]
    fn copies_get_a_meme_name_that_is_not_taken() {
        let dir = std::env::temp_dir().join(format!("sc-meme-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mp4");
        assert_eq!(target(&clip), dir.join("clip-meme.mp4"));
        std::fs::write(dir.join("clip-meme.mp4"), b"").unwrap();
        assert_eq!(target(&clip), dir.join("clip-meme-2.mp4"));
        std::fs::write(dir.join("clip-meme-2.mp4"), b"").unwrap();
        assert_eq!(target(&clip), dir.join("clip-meme-3.mp4"));
        assert_eq!(target(&dir.join("a.mkv")), dir.join("a-meme.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_is_white_with_a_black_outline() {
        let Some(font) = font() else {
            return;
        };
        let (w, h) = (400u32, 200u32);
        let ov = caption(&font, w, h, "hello", "");
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [ov.rgba[i], ov.rgba[i + 3]]
        };
        let white = (0..h / 2)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|(x, y)| px(*x, *y) == [255, 255])
            .count();
        assert!(white > 200, "{white}");
        let (x, y) = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|(x, y)| px(*x, *y) == [255, 255])
            .unwrap();
        let r = stroke(fit("HELLO", w as f32, h as f32, |s, p| advance(&font, s, p)).0);
        let reach = r.ceil() as u32 + 2;
        assert!(y > reach + 2);
        let black = (1..=reach)
            .map(|d| px(x, y - d))
            .any(|p| p[0] < 30 && p[1] > 200);
        assert!(black, "no outline above ({x}, {y})");
        assert_eq!(px(x, y - reach - 2)[1], 0);
        let low = (h / 2..h).flat_map(|y| (0..w).map(move |x| (x, y)));
        assert!(low.into_iter().all(|(x, y)| px(x, y)[1] == 0));
    }

    #[test]
    fn errors_and_paths_are_one_json_line() {
        let v: Value = serde_json::from_str(&reply(Err(EMPTY.into()))).unwrap();
        assert_eq!(v["error"], EMPTY);
        let r = reply(make(Path::new("missing.mp4"), "", ""));
        assert!(!r.contains('\n'));
        assert_eq!(serde_json::from_str::<Value>(&r).unwrap()["error"], EMPTY);
        let r = reply(make(Path::new("missing.mp4"), "TOP", ""));
        assert!(serde_json::from_str::<Value>(&r).unwrap()["error"].is_string());
        let v: Value = serde_json::from_str(&reply(Ok(PathBuf::from(r"C:\a b\c.mp4")))).unwrap();
        assert_eq!(v["open"], r"C:\a b\c.mp4");
    }

    #[test]
    fn the_manifest_parses_with_its_questions() {
        let m: Manifest = serde_json::from_str(include_str!("../manifest.json")).unwrap();
        assert_eq!(m.name, "meme");
        assert_eq!(m.version, VERSION);
        let b = &m.buttons[0];
        assert_eq!((b.id.as_str(), b.place.as_str()), ("caption", "clip"));
        let keys: Vec<_> = b.ask.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, vec!["top", "bottom"]);
        assert_eq!(b.wait_secs(), 600);
    }
}
