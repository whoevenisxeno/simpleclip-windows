use anyhow::{Context, Result};
use interprocess::local_socket::{prelude::*, GenericNamespaced, Stream};
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use std::io::BufReader;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn socket_name() -> String {
    std::env::var("SC_SOCKET").unwrap_or_else(|_| ipc::DEFAULT_SOCKET_NAME.to_string())
}

/// One-shot request to the daemon. The GUI is a client just like `sc`; it never
/// captures. Returns an error (not a panic) if the daemon isn't running, so the
/// UI can show a disconnected state.
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

/// Live connection status the top bar renders.
#[derive(Clone)]
pub enum Link {
    Connected(sc_core::ipc::StatusReport),
    Disconnected,
}

/// Poll the daemon on a background thread and push updates to the UI thread,
/// nudging egui to repaint. One request per second keeps the status bar live
/// without holding a persistent socket open.
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
