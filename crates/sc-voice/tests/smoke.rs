mod common;

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use sc_core::store;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

fn mock_daemon(name: &str) -> (String, Receiver<Option<u32>>) {
    let socket = format!("sc-voice-smoke-{name}-{}.sock", std::process::id());
    let listener = ListenerOptions::new()
        .name(socket.clone().to_ns_name::<GenericNamespaced>().unwrap())
        .create_sync()
        .unwrap();
    let (tx, saves) = mpsc::channel();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let mut reader = BufReader::new(&conn);
            let Ok(Some(env)) = ipc::read_message::<_, Request>(&mut reader, IPC_VERSION) else {
                continue;
            };
            let reply = match env.payload {
                Request::Save { last_secs } => {
                    let _ = tx.send(last_secs);
                    Response::Saved {
                        path: "clip.mp4".into(),
                        duration_secs: 1.0,
                    }
                }
                _ => Response::Ok,
            };
            let mut writer = &conn;
            let _ = ipc::write_message(&mut writer, &Envelope::new(env.id, reply));
        }
    });
    (socket, saves)
}

struct Fed {
    saves: Vec<Option<u32>>,
    status: String,
}

fn feed(root: &Path, name: &str, wav: &Path, settings: &str) -> Fed {
    let dir = root.join(name).join("addons").join("voice");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(store::SETTINGS), settings).unwrap();
    let (socket, rx) = mock_daemon(name);
    let mut child = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .arg("--dir")
        .arg(&dir)
        .arg("--feed-file")
        .arg(wav)
        .arg("--parent")
        .arg(std::process::id().to_string())
        .env("SC_SOCKET", socket)
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > until {
            let _ = child.kill();
            panic!("the add-on did not finish the file");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(200));
    Fed {
        saves: rx.try_iter().collect(),
        status: store::read_status(&dir).map(|s| s.line).unwrap_or_default(),
    }
}

fn wavs(root: &Path) -> Option<Vec<PathBuf>> {
    common::speak(
        root,
        &[
            ("twice", &["simple clip that", "simple clip that"]),
            ("once", &["simpleclip that"]),
            (
                "other",
                &["simple clip is great", "the weather is nice today"],
            ),
        ],
    )
}

#[test]
fn hearing_the_phrase_asks_simpleclip_for_one_clip() {
    let root = common::temp("smoke");
    let Some(w) = wavs(&root) else {
        return;
    };
    let heard = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .arg("--listen-file")
        .arg(&w[0])
        .output()
        .unwrap();
    let heard = String::from_utf8_lossy(&heard.stdout).to_string();
    assert_eq!(
        heard.lines().filter(|l| l.starts_with("yes\t")).count(),
        2,
        "{heard}"
    );
    let fed = feed(&root, "twice", &w[0], r#"{"secs":12,"beep":false}"#);
    assert_eq!(
        fed.saves,
        vec![Some(12)],
        "the cooldown should merge repeats"
    );
    assert!(fed.status.starts_with("Saved a clip"), "{}", fed.status);

    let fed = feed(&root, "once", &w[1], r#"{"beep":false}"#);
    assert_eq!(fed.saves, vec![None]);

    let fed = feed(&root, "other", &w[2], r#"{"beep":false}"#);
    assert!(fed.saves.is_empty(), "{:?}", fed.saves);
    assert_eq!(fed.status, "Listening for \"simple clip that\"");

    let fed = feed(&root, "off", &w[1], r#"{"enabled":false,"beep":false}"#);
    assert!(fed.saves.is_empty(), "{:?}", fed.saves);
    assert_eq!(fed.status, "Off");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_save_that_fails_is_reported_in_the_status() {
    let root = common::temp("nodaemon");
    let Some(w) = common::speak(&root, &[("once", &["simple clip that"])]) else {
        return;
    };
    let dir = root.join("addons").join("voice");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(store::SETTINGS), r#"{"beep":false}"#).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_sc-voice"))
        .arg("--dir")
        .arg(&dir)
        .arg("--feed-file")
        .arg(&w[0])
        .env(
            "SC_SOCKET",
            format!("sc-voice-nobody-{}.sock", std::process::id()),
        )
        .status()
        .unwrap();
    assert!(status.success());
    let line = store::read_status(&dir).unwrap().line;
    assert!(line.contains("could not save"), "{line}");
    let _ = std::fs::remove_dir_all(&root);
}
