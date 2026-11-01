use crate::draw::{self, Image, Lit};
use crate::layout::{Board, DEMO};
use crate::settings::Shape;
use ab_glyph::FontVec;
use std::path::Path;

pub const FILE: &str = "preview.png";

pub fn image(s: &Shape, f: Option<&FontVec>) -> Image {
    let b = Board::new(s.layout, s.mouse);
    let unit = draw::unit_for(s.layout, s.size, 1.0);
    draw::render(
        &b,
        s.theme,
        unit,
        s.opacity as f32 / 100.0,
        f,
        &Lit::demo(&b, DEMO),
    )
}

pub fn write(dir: &Path, s: &Shape, f: Option<&FontVec>) -> std::io::Result<()> {
    let img = image(s, f);
    let tmp = dir.join("preview.png.tmp");
    {
        let file = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        let mut enc = png::Encoder::new(file, img.w as u32, img.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(std::io::Error::other)?;
        w.write_image_data(&img.rgba())
            .map_err(std::io::Error::other)?;
        w.finish().map_err(std::io::Error::other)?;
    }
    std::fs::rename(&tmp, dir.join(FILE))
}
