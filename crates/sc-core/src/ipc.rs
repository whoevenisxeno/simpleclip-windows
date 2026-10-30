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
    GrabRegion {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
    Popup {
        title: String,
        line: String,
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
    Saved {
        path: PathBuf,
        duration_secs: f64,
    },
    Probed {
        duration_secs: f64,
    },
    Thumbnail {
        path: PathBuf,
    },
    Monitors {
        monitors: Vec<MonitorInfo>,
    },
    AudioDevices {
        devices: Vec<AudioDevice>,
    },
    Region {
        width: u32,
        height: u32,
        gray: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tint: Option<String>,
    },
    Ok,
    Error {
        message: String,
    },
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

    #[test]
    fn a_region_without_a_tint_still_reads() {
        let text = r#"{"type":"region","width":1,"height":1,"gray":"AA=="}"#;
        match serde_json::from_str::<Response>(text).unwrap() {
            Response::Region { tint, .. } => assert!(tint.is_none()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_popup_request_round_trips() {
        let req = Request::Popup {
            title: "Game events test".into(),
            line: "Double kill".into(),
        };
        let text = serde_json::to_string(&req).unwrap();
        assert!(text.contains("popup"), "{text}");
        match serde_json::from_str::<Request>(&text).unwrap() {
            Request::Popup { title, line } => assert_eq!(
                (title.as_str(), line.as_str()),
                ("Game events test", "Double kill")
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn grabbing_a_region_round_trips() {
        let req = Request::GrabRegion {
            x0: 0.74,
            y0: 0.06,
            x1: 1.0,
            y1: 0.36,
        };
        let text = serde_json::to_string(&req).unwrap();
        assert!(text.contains("grab_region"), "{text}");
        match serde_json::from_str::<Request>(&text).unwrap() {
            Request::GrabRegion { x0, y1, .. } => assert_eq!((x0, y1), (0.74, 0.36)),
            other => panic!("{other:?}"),
        }
        let reply = Response::Region {
            width: 2,
            height: 1,
            gray: crate::b64::encode(&[10, 200]),
            tint: Some(crate::b64::encode(&[100, 150])),
        };
        let text = serde_json::to_string(&reply).unwrap();
        match serde_json::from_str::<Response>(&text).unwrap() {
            Response::Region {
                width,
                height,
                gray,
                tint,
            } => {
                assert_eq!((width, height), (2, 1));
                assert_eq!(crate::b64::decode(&gray).unwrap(), vec![10, 200]);
                assert_eq!(crate::b64::decode(&tint.unwrap()).unwrap(), vec![100, 150]);
            }
            other => panic!("{other:?}"),
        }
    }
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
