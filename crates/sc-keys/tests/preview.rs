use sc_keys::draw::{self, Frame};
use sc_keys::layout::{Board, Cap, Layout};
use sc_keys::look::Theme;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-keys-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Png {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

fn read(path: &Path) -> Png {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut px = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut px).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    px.truncate(info.buffer_size());
    Png {
        w: info.width as i32,
        h: info.height as i32,
        px,
    }
}

fn shine(img: &Png, fr: &Frame, c: &Cap) -> f32 {
    let r = fr.cap(c);
    let (w, h) = (r[2] - r[0], r[3] - r[1]);
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in r[1] + h * 3 / 10..r[1] + h * 6 / 10 {
        for x in r[0] + w * 12 / 100..r[0] + w * 22 / 100 {
            let i = ((y * img.w + x) * 4) as usize;
            let p = &img.px[i..i + 4];
            sum += p[0].max(p[1]).max(p[2]) as f32 / 255.0 * p[3] as f32 / 255.0;
            n += 1.0;
        }
    }
    sum / n
}

fn preview(dir: &Path, settings: serde_json::Value) -> Png {
    std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
    let _ = std::fs::remove_file(dir.join("preview.png"));
    let status = Command::new(env!("CARGO_BIN_EXE_sc-keys"))
        .arg("--preview-only")
        .arg("--dir")
        .arg(dir)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!dir.join("preview.png.tmp").exists());
    read(&dir.join("preview.png"))
}

#[test]
fn every_layout_and_look_writes_a_good_preview() {
    let dir = temp("preview");
    for layout in Layout::ALL {
        for theme in Theme::ALL {
            let img = preview(
                &dir,
                serde_json::json!({"layout": layout.name(), "theme": theme.name()}),
            );
            let b = Board::new(layout, true);
            let fr = Frame::new(&b, draw::unit_for(layout, 100, 1.0));
            assert_eq!((img.w, img.h), (fr.w, fr.h), "{layout:?} {theme:?}");
            let ratio = img.w as f32 / img.h as f32;
            assert!(
                (ratio - (b.w + 0.5) / (b.h + 0.5)).abs() < 0.05,
                "{layout:?} {ratio}"
            );
            let alphas: Vec<u8> = img.px.chunks(4).map(|p| p[3]).collect();
            assert_eq!(alphas[0], 0, "the corners are see-through");
            assert!(alphas.iter().any(|&a| a > 0 && a < 255));
            assert!(alphas.iter().all(|&a| a <= 230), "opacity 90% applies");
            let find = |l: &str| b.caps.iter().find(|c| c.label == l).unwrap();
            let (hot, cold) = match layout {
                Layout::Arrows => (find("\u{2191}"), find("\u{2193}")),
                _ => (find("W"), find("E")),
            };
            let (h, c) = (shine(&img, &fr, hot), shine(&img, &fr, cold));
            assert!(h > c + 0.2, "{layout:?} {theme:?}: {h} vs {c}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn size_and_mouse_change_the_preview() {
    let dir = temp("size");
    let full = preview(&dir, serde_json::json!({}));
    let wasd = Board::new(Layout::Wasd, true);
    let share = full.w as f32 / 1920.0;
    assert!((0.2..0.24).contains(&share), "{share}");
    let half = preview(&dir, serde_json::json!({"size": 50, "mouse": false}));
    let bare = Board::new(Layout::Wasd, false);
    let fr = Frame::new(&bare, draw::unit_for(Layout::Wasd, 50, 1.0));
    assert_eq!((half.w, half.h), (fr.w, fr.h));
    assert!(half.w < full.w / 2 && wasd.w > bare.w);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_and_uninstall_work() {
    let out = Command::new(env!("CARGO_BIN_EXE_sc-keys"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );
    let status = Command::new(env!("CARGO_BIN_EXE_sc-keys"))
        .arg("--uninstall")
        .status()
        .unwrap();
    assert!(status.success());
}
