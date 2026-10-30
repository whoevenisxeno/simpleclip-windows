use crate::engine::Saver;
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

pub struct DaemonSaver;

impl Saver for DaemonSaver {
    fn save(&mut self, secs: u32) -> Result<(), String> {
        match request(Request::Save {
            last_secs: Some(secs),
        }) {
            Ok(Response::Saved { .. } | Response::Ok) => Ok(()),
            Ok(Response::Error { message }) => Err(message),
            Ok(_) => Err("unexpected answer from SimpleClip".to_string()),
            Err(e) => Err(format!("{e:#}")),
        }
    }

    fn announce(&mut self, title: &str, line: &str) -> Result<(), String> {
        match request(Request::Popup {
            title: title.to_string(),
            line: line.to_string(),
        }) {
            Ok(Response::Ok) => Ok(()),
            Ok(Response::Error { message }) => Err(message),
            Ok(_) => Err("unexpected answer from SimpleClip".to_string()),
            Err(e) => Err(format!("{e:#}")),
        }
    }
}

pub fn grab_region(region: [f32; 4]) -> Result<(u32, u32, Vec<u8>), String> {
    let [x0, y0, x1, y1] = region;
    match request(Request::GrabRegion { x0, y0, x1, y1 }) {
        Ok(Response::Region {
            width,
            height,
            gray,
        }) => sc_core::b64::decode(&gray)
            .filter(|d| d.len() == (width as usize) * (height as usize))
            .map(|d| (width, height, d))
            .ok_or_else(|| "SimpleClip sent a damaged picture".to_string()),
        Ok(Response::Error { message }) => Err(message),
        Ok(_) => Err("unexpected answer from SimpleClip".to_string()),
        Err(e) => Err(format!("{e:#}")),
    }
}
