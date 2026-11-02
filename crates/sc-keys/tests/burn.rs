use sc_ffmpeg::sample::make_clip;
use sc_keys::burn::{burn, Outcome};
use sc_keys::settings::{self, Settings};
use sc_keys::tape::{Event, Input};
use std::path::{Path, PathBuf};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-keys-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn corner(path: &Path, at: f64) -> f64 {
    let (w, h, rgb) = sc_ffmpeg::frame_at(path, at, 640).unwrap();
    let (w, h) = (w as usize, h as usize);
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in h * 6 / 10..h {
        for x in 0..w * 3 / 10 {
            let p = &rgb[(y * w + x) * 3..][..3];
            sum += p.iter().map(|v| *v as f64).sum::<f64>() / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

fn settings() -> Settings {
    let mut s = Settings::read(&settings::manifest(), &serde_json::Map::new());
    s.at = [0.0, 1.0];
    s.size = 200;
    s.opacity = 100;
    s
}

#[test]
fn keys_pressed_during_a_clip_are_drawn_into_it_and_the_sound_stays() {
    let dir = temp("drawn");
    let clip = dir.join("clip.mp4");
    if !make_clip(&clip, 4, 640, 360, true) {
        return;
    }
    let original = dir.join("original.mp4");
    std::fs::copy(&clip, &original).unwrap();
    let start = 1_000.0;
    let events = [
        Event {
            at: start + 2.0,
            input: Input::Key(b'W'),
            down: true,
        },
        Event {
            at: start + 3.0,
            input: Input::Key(b'W'),
            down: false,
        },
    ];
    let out = burn(&clip, start, &events, &settings(), None, &dir.join("work")).unwrap();
    assert_eq!(out, Outcome::Added);
    let before = sc_ffmpeg::media_info(&original).unwrap();
    let after = sc_ffmpeg::media_info(&clip).unwrap();
    assert_eq!(after.audio_streams, before.audio_streams);
    assert_eq!((after.width, after.height), (before.width, before.height));
    assert!((after.duration - before.duration).abs() < 0.15);
    let changed = |at: f64| (corner(&clip, at) - corner(&original, at)).abs();
    assert!(changed(1.0) > 4.0, "the board shows all the time");
    let pressed = corner(&clip, 2.5) - corner(&original, 2.5);
    let idle = corner(&clip, 1.0) - corner(&original, 1.0);
    assert!(
        (pressed - idle).abs() > 1.0,
        "a held key looks different, pressed {pressed} idle {idle}"
    );
    assert!(!dir.join("work").join("joined.mp4").exists());
    assert!(!dir.join("work").join("picture.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_clip_with_no_key_presses_is_left_untouched() {
    let dir = temp("quiet");
    let clip = dir.join("clip.mp4");
    if !make_clip(&clip, 2, 320, 180, true) {
        return;
    }
    let bytes = std::fs::read(&clip).unwrap();
    let events = [Event {
        at: 10.0,
        input: Input::Key(b'W'),
        down: false,
    }];
    let out = burn(&clip, 500.0, &events, &settings(), None, &dir.join("work")).unwrap();
    assert_eq!(out, Outcome::Quiet);
    assert_eq!(std::fs::read(&clip).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}
