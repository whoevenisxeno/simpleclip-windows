use anyhow::{Context, Result};
use interprocess::local_socket::{prelude::*, GenericNamespaced, Stream};
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use std::io::BufReader;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn socket_name() -> String {
    std::env::var("SC_SOCKET").unwrap_or_else(|_| ipc::DEFAULT_SOCKET_NAME.to_string())
}

/// One request, one response. The CLI is one-shot; the GUI keeps a connection
/// open, but both use this same envelope protocol.
pub fn request(req: Request) -> Result<Response> {
    let name = socket_name();
    let ns = name.to_ns_name::<GenericNamespaced>()?;
    let stream = Stream::connect(ns).context("cannot reach scd (is the daemon running?)")?;
    let mut writer = &stream;
    let mut reader = BufReader::new(&stream);

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    ipc::write_message(&mut writer, &Envelope::new(id, req))?;
    let env: Envelope<Response> = ipc::read_message(&mut reader, IPC_VERSION)?
        .context("daemon closed the connection without responding")?;
    Ok(env.payload)
}
