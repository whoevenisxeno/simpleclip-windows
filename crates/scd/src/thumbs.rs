use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "SimpleClip")
        .map(|d| d.data_local_dir().join("thumbs"))
        .unwrap_or_else(|| PathBuf::from("thumbs"))
}

fn cache_name(path: &Path, width: u32, at_tenths: Option<i64>) -> String {
    let meta = std::fs::metadata(path).ok();
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    width.hash(&mut h);
    at_tenths.hash(&mut h);
    if let Some(m) = &meta {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    format!("{:016x}.png", h.finish())
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_png(out: &Path, w: u32, h: u32, rgb: &[u8]) -> Result<(), String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = out.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let file = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header()
            .and_then(|mut wr| wr.write_image_data(rgb))
            .map_err(|e| e.to_string())?;
    }
    match std::fs::rename(&tmp, out) {
        Ok(()) => Ok(()),
        Err(_) if out.exists() => {
            let _ = std::fs::remove_file(&tmp);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.to_string())
        }
    }
}

#[cfg(any(target_os = "linux", windows))]
pub fn cached_thumbnail(path: &Path, width: u32) -> Result<PathBuf, String> {
    let width = width.clamp(64, 1280);
    let out = cache_dir().join(cache_name(path, width, None));
    if out.exists() {
        return Ok(out);
    }
    let (w, h, rgb) = sc_ffmpeg::thumbnail(path, width).map_err(|e| e.to_string())?;
    write_png(&out, w, h, &rgb)?;
    Ok(out)
}

#[cfg(any(target_os = "linux", windows))]
pub fn cached_frame(path: &Path, secs: f64, width: u32) -> Result<PathBuf, String> {
    let width = width.clamp(64, 1920);
    let tenths = (secs.max(0.0) * 10.0).round() as i64;
    let out = cache_dir().join(cache_name(path, width, Some(tenths)));
    if out.exists() {
        return Ok(out);
    }
    let (w, h, rgb) =
        sc_ffmpeg::frame_at(path, tenths as f64 / 10.0, width).map_err(|e| e.to_string())?;
    write_png(&out, w, h, &rgb)?;
    Ok(out)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn cached_thumbnail(_: &Path, _: u32) -> Result<PathBuf, String> {
    Err("not supported on this platform".into())
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn cached_frame(_: &Path, _: f64, _: u32) -> Result<PathBuf, String> {
    Err("not supported on this platform".into())
}

pub fn prune() {
    const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);
    const MAX_BYTES: u64 = 200 * 1024 * 1024;
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for e in entries.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() {
            continue;
        }
        let when = m.modified().unwrap_or(now);
        if now.duration_since(when).unwrap_or_default() > MAX_AGE {
            let _ = std::fs::remove_file(e.path());
        } else {
            files.push((when, m.len(), e.path()));
        }
    }
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= MAX_BYTES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_name_depends_on_width_and_time() {
        let p = Path::new("a.mp4");
        assert_ne!(cache_name(p, 320, None), cache_name(p, 640, None));
        assert_ne!(cache_name(p, 320, Some(10)), cache_name(p, 320, Some(11)));
        assert_ne!(cache_name(p, 320, None), cache_name(p, 320, Some(0)));
        assert_eq!(cache_name(p, 320, None), cache_name(p, 320, None));
    }

    #[test]
    fn concurrent_writers_do_not_clash() {
        let dir = std::env::temp_dir().join(format!("sc-thumb-test-{}", std::process::id()));
        let out = dir.join("x.png");
        let rgb = vec![200u8; 8 * 8 * 3];
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (o, r) = (out.clone(), rgb.clone());
                std::thread::spawn(move || write_png(&o, 8, 8, &r))
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().is_ok());
        }
        assert!(out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
