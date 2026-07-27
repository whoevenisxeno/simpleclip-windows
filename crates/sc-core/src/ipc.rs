use crate::audio::AudioDevice;
use crate::capture::{CaptureState, MonitorInfo};
use crate::encode::EncoderKind;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Bump on any breaking change to the message shapes below. Once published,
/// only additive changes are allowed within a version (§13).
pub const IPC_VERSION: u32 = 1;

/// Default socket/pipe name, overridable via `SC_SOCKET` for tests and
/// side-by-side daemons.
pub const DEFAULT_SOCKET_NAME: &str = "simpleclip.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    Save { last_secs: Option<u32> },
    Screenshot,
    Record,
    Stop,
    Pause,
    Resume,
    ListMonitors,
    ListAudioDevices,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub state: CaptureState,
    pub recording: bool,
    pub buffer_secs: u32,
    pub buffer_fill: f32,
    pub monitor: Option<MonitorInfo>,
    pub encoder: Option<EncoderKind>,
    pub drift_ms: f64,
    pub daemon_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Status(StatusReport),
    Saved { path: PathBuf, duration_secs: f64 },
    Monitors(Vec<MonitorInfo>),
    AudioDevices(Vec<AudioDevice>),
    Ok,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub version: u32,
    pub id: u64,
    pub payload: T,
}

impl<T> Envelope<T> {
    pub fn new(id: u64, payload: T) -> Self {
        Self {
            version: IPC_VERSION,
            id,
            payload,
        }
    }
}

/// Write one newline-delimited JSON message. Framing is a trailing '\n'; the
/// version field inside the envelope guards against schema drift.
pub fn write_message<W: Write, T: Serialize>(w: &mut W, msg: &Envelope<T>) -> Result<()> {
    let mut line = serde_json::to_vec(msg)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()?;
    Ok(())
}

/// Read one newline-delimited JSON message and check the protocol version.
pub fn read_message<R: BufRead, T: for<'de> Deserialize<'de>>(
    r: &mut R,
    expected_version: u32,
) -> Result<Option<Envelope<T>>> {
    let mut line = String::new();
    let n = r.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let env: Envelope<T> = serde_json::from_str(line.trim_end())?;
    if env.version != expected_version {
        return Err(Error::IpcVersion {
            client: env.version,
            daemon: expected_version,
        });
    }
    Ok(Some(env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn request_roundtrips_over_wire() {
        let mut buf = Vec::new();
        let req = Envelope::new(
            7,
            Request::Save {
                last_secs: Some(30),
            },
        );
        write_message(&mut buf, &req).unwrap();
        let mut reader = BufReader::new(&buf[..]);
        let got: Envelope<Request> = read_message(&mut reader, IPC_VERSION).unwrap().unwrap();
        assert_eq!(got.id, 7);
        matches!(
            got.payload,
            Request::Save {
                last_secs: Some(30)
            }
        );
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let mut buf = Vec::new();
        write_message(&mut buf, &Envelope::new(1, Request::Ping)).unwrap();
        let mut reader = BufReader::new(&buf[..]);
        let err = read_message::<_, Request>(&mut reader, IPC_VERSION + 1);
        assert!(matches!(err, Err(Error::IpcVersion { .. })));
    }
}
