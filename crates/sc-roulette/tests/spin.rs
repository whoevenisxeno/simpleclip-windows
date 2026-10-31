use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_sc-roulette");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-roulette-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> String {
    let out = Command::new(EXE).args(args).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn spin(dir: &Path) -> Value {
    let text = run(&["--button", "spin", "--clips", dir.to_str().unwrap()]);
    assert_eq!(text.trim_end().lines().count(), 1);
    serde_json::from_str(text.trim()).unwrap()
}

#[test]
fn spinning_opens_a_real_clip_or_says_there_is_none() {
    let dir = temp("spin");
    assert!(spin(&dir)["toast"].is_string());
    std::fs::create_dir_all(dir.join("game")).unwrap();
    std::fs::write(dir.join("game/small.mp4"), [0u8; 10]).unwrap();
    std::fs::write(dir.join("game/clip-trim.mp4"), vec![0u8; 200_000]).unwrap();
    std::fs::write(dir.join("game/clip-8mb.mp4"), vec![0u8; 200_000]).unwrap();
    assert!(spin(&dir)["toast"].is_string());
    let real = dir.join("game/clip.mp4");
    std::fs::write(&real, vec![0u8; 200_000]).unwrap();
    for _ in 0..3 {
        let v = spin(&dir);
        let got = PathBuf::from(v["open"].as_str().unwrap());
        assert!(got.is_absolute());
        assert_eq!(got.canonicalize().unwrap(), real.canonicalize().unwrap());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_and_uninstall_work() {
    assert_eq!(run(&["--version"]).trim(), env!("CARGO_PKG_VERSION"));
    assert_eq!(run(&["--uninstall"]), "");
}
