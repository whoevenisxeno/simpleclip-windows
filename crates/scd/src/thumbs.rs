//! Gallery preview images, generated on demand and cached on disk.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "SimpleClip")
        .map(|d| d.data_local_dir().join("thumbs"))
        .unwrap_or_else(|| PathBuf::from("thumbs"))
}

/// Cache key: path plus size and mtime, so an edited or replaced file gets a
/// fresh preview.
fn cache_name(path: &Path, width: u32) -> String {
    let meta = std::fs::metadata(path).ok();
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    width.hash(&mut h);
    if let Some(m) = &meta {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    format!("{:016x}.png", h.finish())
}

/// Return the path of a preview PNG for `path`, creating it if needed.
#[cfg(any(target_os = "linux", windows))]
pub fn cached_thumbnail(path: &Path, width: u32) -> Result<PathBuf, String> {
    let width = width.clamp(64, 1280);
    let dir = cache_dir();
    let out = dir.join(cache_name(path, width));
    if out.exists() {
        return Ok(out);
    }
    let (w, h, rgb) = sc_ffmpeg::thumbnail(path, width).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Write to a temp name first so a half-written file is never served.
    let tmp = out.with_extension("tmp");
    {
        let file = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()
            .and_then(|mut wr| wr.write_image_data(&rgb))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn cached_thumbnail(_: &Path, _: u32) -> Result<PathBuf, String> {
    Err("not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_name_depends_on_width() {
        let p = Path::new("a.mp4");
        assert_ne!(cache_name(p, 320), cache_name(p, 640));
        assert_eq!(cache_name(p, 320), cache_name(p, 320));
    }
}
