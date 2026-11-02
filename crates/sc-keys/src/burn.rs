use crate::draw::{self, Lit};
use crate::layout::Board;
use crate::place::{self, Rect};
use crate::settings::Settings;
use crate::tape::{self, Event, Player};
use ab_glyph::FontVec;
use sc_core::edit::Plan;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const REPLACE_WAIT: Duration = Duration::from_secs(60);
const SIDE: &str = "keys-part";

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Added,
    Beside(PathBuf),
    Quiet,
}

fn plain(e: sc_ffmpeg::Error) -> String {
    let e = e.to_string();
    e.trim_start_matches("ffmpeg: ").to_string()
}

pub fn scale_for(height: u32) -> f32 {
    (height as f32 / 1080.0).clamp(0.25, 4.0)
}

struct Sprite {
    lit: Lit,
    x: i32,
    y: i32,
    stamp: sc_ffmpeg::Stamp,
}

pub fn burn(
    clip: &Path,
    start: f64,
    events: &[Event],
    s: &Settings,
    font: Option<&FontVec>,
    work: &Path,
) -> Result<Outcome, String> {
    let info = sc_ffmpeg::media_info(clip).map_err(plain)?;
    if !tape::pressed_during(events, start, start + info.duration) {
        return Ok(Outcome::Quiet);
    }
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let picture = work.join("picture.mp4");
    let joined = work.join("joined.mp4");
    let board = Board::new(s.layout, s.mouse);
    let unit = draw::unit_for(s.layout, s.size, scale_for(info.height));
    let opacity = s.opacity as f32 / 100.0;
    let mut player = Player::new(events);
    let mut sprite: Option<Sprite> = None;
    let mut paint = |t: f64, c: &mut sc_ffmpeg::Canvas| {
        let lit = player.at(&board, start + t);
        if sprite.as_ref().is_none_or(|sp| sp.lit != lit) {
            let img = draw::render(&board, s.theme, unit, opacity, font, &lit);
            let screen = Rect::new(0, 0, c.width as i32, c.height as i32);
            let (x, y) = place::place(screen, (img.w, img.h), s.at);
            sprite = Some(Sprite {
                lit,
                x,
                y,
                stamp: sc_ffmpeg::Stamp::from_rgba(img.w as usize, img.h as usize, &img.rgba()),
            });
        }
        if let Some(sp) = &sprite {
            c.stamp(sp.x as i64, sp.y as i64, &sp.stamp);
        }
    };
    let mut plan = Plan::whole(info.duration);
    plan.volume = 0.0;
    let done = sc_ffmpeg::render_painted(
        clip,
        &picture,
        &plan,
        &sc_ffmpeg::EditProgress::default(),
        Some(&mut paint),
    )
    .map_err(plain)
    .and_then(|_| sc_ffmpeg::with_sound_of(&picture, clip, &joined).map_err(plain));
    let _ = std::fs::remove_file(&picture);
    if let Err(e) = done {
        let _ = std::fs::remove_file(&joined);
        return Err(e);
    }
    let out = replace(clip, &joined, REPLACE_WAIT);
    let _ = std::fs::remove_file(&joined);
    out
}

pub fn report(done: &Result<Outcome, String>) -> String {
    let one = |s: &str| s.replace(['\n', '\r', '\t'], " ");
    match done {
        Ok(Outcome::Added) => "added".into(),
        Ok(Outcome::Quiet) => "quiet".into(),
        Ok(Outcome::Beside(p)) => format!("beside\t{}", one(&p.to_string_lossy())),
        Err(e) => format!("error\t{}", one(e)),
    }
}

pub fn read_report(line: &str) -> Result<Outcome, String> {
    let line = line.trim();
    let (kind, rest) = line.split_once('\t').unwrap_or((line, ""));
    match kind {
        "added" => Ok(Outcome::Added),
        "quiet" => Ok(Outcome::Quiet),
        "beside" => Ok(Outcome::Beside(PathBuf::from(rest))),
        "error" => Err(rest.to_string()),
        _ => Err("the helper stopped without an answer".into()),
    }
}

fn side_of(clip: &Path) -> PathBuf {
    let mut name = clip.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(SIDE);
    clip.with_file_name(name)
}

pub fn beside(clip: &Path) -> PathBuf {
    let stem = clip
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "clip".into());
    (0..)
        .map(|n| match n {
            0 => clip.with_file_name(format!("{stem}-keys.mp4")),
            n => clip.with_file_name(format!("{stem}-keys-{n}.mp4")),
        })
        .find(|p| !p.exists())
        .unwrap()
}

pub fn replace(clip: &Path, new: &Path, wait: Duration) -> Result<Outcome, String> {
    let side = side_of(clip);
    if std::fs::rename(new, &side).is_err() {
        std::fs::copy(new, &side).map_err(|e| format!("could not write next to the clip: {e}"))?;
    }
    let until = Instant::now() + wait;
    loop {
        match std::fs::rename(&side, clip) {
            Ok(()) => return Ok(Outcome::Added),
            Err(_) if Instant::now() < until && clip.exists() => {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(_) => {
                let to = beside(clip);
                return match std::fs::rename(&side, &to) {
                    Ok(()) => Ok(Outcome::Beside(to)),
                    Err(e) => {
                        let _ = std::fs::remove_file(&side);
                        Err(format!("could not save the clip with keys: {e}"))
                    }
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-keys-burn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_new_file_takes_the_clips_place() {
        let dir = temp("replace");
        let clip = dir.join("clip.mp4");
        let new = dir.join("work.mp4");
        std::fs::write(&clip, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        assert_eq!(
            replace(&clip, &new, Duration::ZERO).unwrap(),
            Outcome::Added
        );
        assert_eq!(std::fs::read(&clip).unwrap(), b"new");
        assert!(!side_of(&clip).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_clip_held_open_gets_a_copy_beside_it() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp("busy");
        let clip = dir.join("clip.mp4");
        let new = dir.join("work.mp4");
        std::fs::write(&clip, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&clip)
            .unwrap();
        let out = replace(&clip, &new, Duration::from_millis(600)).unwrap();
        drop(held);
        assert_eq!(out, Outcome::Beside(dir.join("clip-keys.mp4")));
        assert_eq!(std::fs::read(&clip).unwrap(), b"old");
        assert_eq!(std::fs::read(dir.join("clip-keys.mp4")).unwrap(), b"new");
        assert!(!side_of(&clip).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_beside_get_a_free_name() {
        let dir = temp("names");
        let clip = dir.join("Game 1.mp4");
        assert_eq!(beside(&clip), dir.join("Game 1-keys.mp4"));
        std::fs::write(dir.join("Game 1-keys.mp4"), b"x").unwrap();
        assert_eq!(beside(&clip), dir.join("Game 1-keys-1.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn answers_from_the_helper_read_back() {
        for done in [
            Ok(Outcome::Added),
            Ok(Outcome::Quiet),
            Ok(Outcome::Beside(PathBuf::from(r"C:\clips\a b-keys.mp4"))),
            Err("no video stream".to_string()),
        ] {
            assert_eq!(read_report(&format!("{}\r\n", report(&done))), done);
        }
        assert_eq!(read_report(&report(&Err("a\nb".into()))), Err("a b".into()));
        assert!(read_report("").is_err());
    }

    #[test]
    fn the_size_follows_the_clip_height() {
        assert_eq!(scale_for(1080), 1.0);
        assert_eq!(scale_for(2160), 2.0);
        assert!((scale_for(720) - 0.6667).abs() < 0.001);
    }
}
