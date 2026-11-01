use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_sc-meme");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-meme-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> String {
    let out = Command::new(EXE).args(args).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn caption(clip: &Path, dir: &Path, top: &str, bottom: &str) -> Value {
    let (top, bottom) = (format!("top={top}"), format!("bottom={bottom}"));
    let text = run(&[
        "--button",
        "caption",
        "--clip",
        clip.to_str().unwrap(),
        "--clips",
        dir.to_str().unwrap(),
        "--set",
        &top,
        "--set",
        &bottom,
    ]);
    assert_eq!(text.trim_end().lines().count(), 1, "{text}");
    serde_json::from_str(text.trim()).unwrap()
}

fn top_rows(path: &Path) -> Vec<u8> {
    let (w, h, rgb) = sc_ffmpeg::frame_at(path, 1.0, 640).unwrap();
    rgb[..(w * h / 5 * 3) as usize].to_vec()
}

#[test]
fn a_meme_copy_has_text_on_top_and_keeps_the_clip_shape() {
    let dir = temp("caption");
    let clip = dir.join("clip.mp4");
    if !sc_ffmpeg::sample::make_clip(&clip, 3, 640, 360, true) {
        return;
    }
    let v = caption(&clip, &dir, "TOP", "BOTTOM");
    let out = PathBuf::from(v["open"].as_str().unwrap_or_else(|| panic!("{v}")));
    assert!(out.is_absolute() && out.is_file());
    assert_eq!(out.file_name().unwrap(), "clip-meme.mp4");
    let (a, b) = (
        sc_ffmpeg::media_info(&clip).unwrap(),
        sc_ffmpeg::media_info(&out).unwrap(),
    );
    assert_eq!((a.width, a.height), (b.width, b.height));
    assert!(
        (a.duration - b.duration).abs() < 0.25,
        "{} {}",
        a.duration,
        b.duration
    );
    assert_eq!(b.audio_streams, 1);
    let (was, now) = (top_rows(&clip), top_rows(&out));
    let changed = was
        .iter()
        .zip(&now)
        .filter(|(x, y)| x.abs_diff(**y) > 60)
        .count();
    assert!(changed > now.len() / 50, "{changed} of {}", now.len());

    let again = caption(&clip, &dir, "", "ONLY BOTTOM");
    let second = PathBuf::from(again["open"].as_str().unwrap());
    assert_eq!(second.file_name().unwrap(), "clip-meme-2.mp4");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn problems_come_back_as_an_error_line() {
    let dir = temp("errors");
    let v = caption(&dir.join("gone.mp4"), &dir, "TOP", "");
    assert!(v["error"].is_string());
    let v = caption(&dir.join("gone.mp4"), &dir, " ", "");
    assert!(v["error"].is_string());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_and_uninstall_work() {
    assert_eq!(run(&["--version"]).trim(), "1.0.0");
    assert_eq!(run(&["--uninstall"]), "");
}
