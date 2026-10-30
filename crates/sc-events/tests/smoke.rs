use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions};
use sc_core::addon;
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const ME: &str = "76561198000000001";

struct Running {
    child: Child,
    dir: PathBuf,
    port: u16,
    saves: Receiver<Option<u32>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(name: &str, events: &str) -> Running {
    let dir = std::env::temp_dir().join(format!("sc-events-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    std::fs::write(&config, format!("[events]\n{events}\n")).unwrap();

    let socket = format!("sc-events-smoke-{name}-{}.sock", std::process::id());
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
            let secs = match env.payload {
                Request::Save { last_secs } => last_secs,
                _ => None,
            };
            let _ = tx.send(secs);
            let reply = Response::Saved {
                path: "clip.mp4".into(),
                duration_secs: 1.0,
            };
            let mut writer = &conn;
            let _ = ipc::write_message(&mut writer, &Envelope::new(env.id, reply));
        }
    });

    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_sc-events"))
        .arg("--dir")
        .arg(&dir)
        .arg("--config")
        .arg(&config)
        .args(["--port", &port.to_string(), "--no-cfg", "--parent"])
        .arg(std::process::id().to_string())
        .env("SC_SOCKET", socket)
        .spawn()
        .unwrap();
    Running {
        child,
        dir,
        port,
        saves,
    }
}

fn wait_for_status(
    run: &Running,
    what: impl Fn(&addon::EventsStatus) -> bool,
) -> addon::EventsStatus {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(s) = addon::read_status(&run.dir).filter(&what) {
            return s;
        }
        assert!(
            Instant::now() < until,
            "the add-on never reported the expected status"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn post(port: u16, round: i64, kills: u32) {
    let body = format!(
        r#"{{"provider":{{"steamid":"{ME}"}},"map":{{"round":{round}}},"player":{{"steamid":"{ME}","state":{{"round_kills":{kills}}}}}}}"#
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let request = format!(
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
}

#[test]
fn a_double_kill_makes_the_running_add_on_ask_simpleclip_for_a_clip() {
    let run = start(
        "on",
        "enabled = true\nmin-kills = 2\nsave-secs = 12\ncs2 = true\nleague = false",
    );
    let status = wait_for_status(&run, |s| s.cs2 == "not-found");
    assert_eq!(status.league, "off");
    assert!(!status.version.is_empty());
    for kills in [0, 1, 2] {
        post(run.port, 3, kills);
    }
    let asked = run.saves.recv_timeout(Duration::from_secs(15)).unwrap();
    assert_eq!(asked, Some(12));
    let status = wait_for_status(&run, |s| s.last_event.is_some());
    let event = status.last_event.unwrap();
    assert!(event.contains("Double kill in CS2"), "{event}");
    assert!(event.contains("clip saved"), "{event}");
    assert!(run.saves.recv_timeout(Duration::from_secs(6)).is_err());
}

#[test]
fn nothing_is_saved_while_the_add_on_is_switched_off() {
    let run = start("off", "enabled = false");
    wait_for_status(&run, |s| s.cs2 == "off");
    for kills in [0, 1, 2, 3] {
        post(run.port, 3, kills);
    }
    assert!(run.saves.recv_timeout(Duration::from_secs(7)).is_err());
}

#[test]
fn the_add_on_exits_when_the_program_that_started_it_is_gone() {
    let mut parent = Command::new("cmd")
        .args(["/C", "ping -n 4 127.0.0.1 >nul"])
        .spawn()
        .unwrap();
    let dir = std::env::temp_dir().join(format!("sc-events-orphan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    std::fs::write(&config, "[events]\nenabled = false\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sc-events"))
        .arg("--dir")
        .arg(&dir)
        .arg("--config")
        .arg(&config)
        .args(["--port", &free_port().to_string(), "--no-cfg", "--parent"])
        .arg(parent.id().to_string())
        .spawn()
        .unwrap();
    parent.wait().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    let exited = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if Instant::now() > until {
            break false;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !exited {
        let _ = child.kill();
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(exited, "the add-on kept running without its parent");
}
