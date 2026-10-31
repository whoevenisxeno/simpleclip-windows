mod common;

use std::process::Command;

fn triggers(wav: &std::path::Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .arg("--listen-file")
        .arg(wav)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (text.lines().any(|l| l.starts_with("yes\t")), text)
}

#[test]
fn the_phrase_is_heard_and_other_sentences_are_not() {
    let dir = common::temp("listen");
    let items: [(&str, &[&str]); 7] = [
        ("spaced", &["simple clip that"]),
        ("joined", &["simpleclip that"]),
        ("great", &["simple clip is great"]),
        ("this", &["simple clip this"]),
        ("lot", &["I like simple clip a lot"]),
        ("trip", &["simple trip that"]),
        ("weather", &["the weather is nice today"]),
    ];
    let Some(wavs) = common::speak(&dir, &items) else {
        return;
    };
    for ((name, _), wav) in items.iter().zip(&wavs) {
        let (yes, text) = triggers(wav);
        let want = matches!(*name, "spaced" | "joined");
        assert_eq!(yes, want, "{name}: {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_an_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .args(["--listen-file", "definitely-not-here.wav"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
}

#[test]
fn version_and_uninstall_work() {
    let out = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );
    let status = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .arg("--uninstall")
        .status()
        .unwrap();
    assert!(status.success());
}
