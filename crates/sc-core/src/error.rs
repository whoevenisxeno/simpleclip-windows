use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("config file not found at {0}")]
    ConfigMissing(PathBuf),

    #[error("ipc protocol error: {0}")]
    Ipc(String),

    #[error("ipc version mismatch: client={client}, daemon={daemon}")]
    IpcVersion { client: u32, daemon: u32 },

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("no capture backend available for this platform/session")]
    NoCaptureBackend,

    #[error(
        "no hardware encoder available; enable the `software-encode` feature to use CPU encode"
    )]
    NoEncoder,

    #[error("capture is not running")]
    NotCapturing,

    #[error("{0}")]
    Other(String),
}
