use crate::audio::AudioDevice;
use crate::capture::{CaptureState, MonitorInfo};
use crate::encode::EncoderKind;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const IPC_VERSION: u32 = 1;

pub const DEFAULT_SOCKET_NAME: &str = "simpleclip.sock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    Save {
        last_secs: Option<u32>,
    },
    Screenshot,
    Record,
    Stop,
    Pause,
    Resume,
    ListMonitors,
    ListAudioDevices,
    Shutdown,
    TestNotification,
    Thumbnail {
        path: PathBuf,
        width: u32,
    },
    Probe {
        path: PathBuf,
    },
    Frame {
        path: PathBuf,
        secs: f64,
        width: u32,
    },
    Trim {
        path: PathBuf,
        start_secs: f64,
        end_secs: f64,
        #[serde(default)]
        dest: TrimDest,
    },
    Compress {
        path: PathBuf,
        target_mb: u32,
        #[serde(default)]
        max_height: Option<u32>,
        #[serde(default)]
        max_fps: Option<u32>,
        #[serde(default)]
        mute: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimDest {
    #[default]
    Default,
    Beside,
    Temp,
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
    Probed { duration_secs: f64 },
    Thumbnail { path: PathBuf },
    Monitors { monitors: Vec<MonitorInfo> },
    AudioDevices { devices: Vec<AudioDevice> },
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

pub fn write_message<W: Write, T: Serialize>(w: &mut W, msg: &Envelope<T>) -> Result<()> {
    let mut line = serde_json::to_vec(msg)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()?;
    Ok(())
}

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
    fn an_old_compress_request_still_reads_and_gets_plain_defaults() {
        let old = r#"{"version":1,"id":3,"payload":{"cmd":"compress","path":"C:/Clips/a.mp4","target_mb":25}}"#;
        let mut reader = BufReader::new(old.as_bytes());
        let got: Envelope<Request> = read_message(&mut reader, IPC_VERSION).unwrap().unwrap();
        match got.payload {
            Request::Compress {
                target_mb,
                max_height,
                max_fps,
                mute,
                ..
            } => {
                assert_eq!(target_mb, 25);
                assert_eq!((max_height, max_fps, mute), (None, None, false));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_new_compress_request_roundtrips_with_its_options() {
        let mut buf = Vec::new();
        let req = Envelope::new(
            4,
            Request::Compress {
                path: "C:/Clips/a.mp4".into(),
                target_mb: 50,
                max_height: Some(720),
                max_fps: Some(30),
                mute: true,
            },
        );
        write_message(&mut buf, &req).unwrap();
        let mut reader = BufReader::new(&buf[..]);
        let got: Envelope<Request> = read_message(&mut reader, IPC_VERSION).unwrap().unwrap();
        assert!(matches!(
            got.payload,
            Request::Compress {
                target_mb: 50,
                max_height: Some(720),
                max_fps: Some(30),
                mute: true,
                ..
            }
        ));
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
