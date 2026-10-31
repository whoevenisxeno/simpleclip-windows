use anyhow::{Context, Result};
use interprocess::local_socket::{prelude::*, GenericNamespaced, Stream};
use sc_core::ipc::{self, Envelope, Request, Response, IPC_VERSION};
use std::io::BufReader;

fn socket_name() -> String {
    std::env::var("SC_SOCKET").unwrap_or_else(|_| ipc::DEFAULT_SOCKET_NAME.to_string())
}

fn request(req: Request) -> Result<Response> {
    let ns = socket_name().to_ns_name::<GenericNamespaced>()?;
    let stream = Stream::connect(ns).context("SimpleClip is not running")?;
    let mut writer = &stream;
    let mut reader = BufReader::new(&stream);
    ipc::write_message(&mut writer, &Envelope::new(1, req))?;
    let env: Envelope<Response> =
        ipc::read_message(&mut reader, IPC_VERSION)?.context("SimpleClip did not answer")?;
    Ok(env.payload)
}

pub fn save(last_secs: Option<u32>) -> Result<(), String> {
    match request(Request::Save { last_secs }) {
        Ok(Response::Saved { .. } | Response::Ok) => Ok(()),
        Ok(Response::Error { message }) => Err(message),
        Ok(_) => Err("unexpected answer from SimpleClip".to_string()),
        Err(e) => Err(format!("{e:#}")),
    }
}
