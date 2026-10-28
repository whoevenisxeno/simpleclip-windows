use anyhow::{Context, Result};
use interprocess::local_socket::{prelude::*, GenericNamespaced, Stream};
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use std::io::BufReader;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn socket_name() -> String {
    std::env::var("SC_SOCKET").unwrap_or_else(|_| ipc::DEFAULT_SOCKET_NAME.to_string())
}

pub fn request(req: Request) -> Result<Response> {
    let ns = socket_name().to_ns_name::<GenericNamespaced>()?;
    let stream = Stream::connect(ns).context("daemon not reachable")?;
    let mut writer = &stream;
    let mut reader = BufReader::new(&stream);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    ipc::write_message(&mut writer, &Envelope::new(id, req))?;
    let env: Envelope<Response> =
        ipc::read_message(&mut reader, IPC_VERSION)?.context("daemon closed connection")?;
    Ok(env.payload)
}

pub fn spawn_daemon() -> bool {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent()
                .map(|d| d.join(format!("scd{}", std::env::consts::EXE_SUFFIX)))
        })
        .filter(|p| p.exists());
    let mut cmd = std::process::Command::new(sibling.unwrap_or_else(|| "scd".into()));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    cmd.spawn().is_ok()
}

pub fn ensure_running() {
    if request(Request::Ping).is_err() {
        spawn_daemon();
    }
}

#[derive(Clone)]
pub enum Link {
    Connected(sc_core::ipc::StatusReport),
    Disconnected,
}

pub fn spawn_status_poller(ctx: egui::Context, tx: crossbeam_channel::Sender<Link>) {
    std::thread::spawn(move || loop {
        let link = match request(Request::Status) {
            Ok(Response::Status(s)) => Link::Connected(s),
            _ => Link::Disconnected,
        };
        if tx.send(link).is_err() {
            break;
        }
        ctx.request_repaint();
        std::thread::sleep(std::time::Duration::from_secs(1));
    });
}
