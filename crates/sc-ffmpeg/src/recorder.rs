use crate::encoder::Encoder;
use crate::mux::Mp4Muxer;
use crate::Result;

/// Convenience wrapper that encodes NV12 frames straight to an MP4 file (manual
/// recording). The instant-replay path instead keeps the `Encoder` feeding a ring
/// buffer and builds an `Mp4Muxer` only when a clip is saved.
pub struct Recorder {
    enc: Encoder,
    mux: Option<Mp4Muxer>,
}

impl Recorder {
    pub fn new(
        path: &std::path::Path,
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        gop_frames: u32,
    ) -> Result<Self> {
        let enc = Encoder::new(width, height, fps, bitrate_kbps, gop_frames)?;
        let mux = Mp4Muxer::new(path, &enc)?;
        Ok(Self {
            enc,
            mux: Some(mux),
        })
    }

    pub fn push_nv12(&mut self, nv12: &[u8], pts_nanos: i64) -> Result<()> {
        let packets = self.enc.push_nv12(nv12, pts_nanos)?;
        if let Some(mux) = self.mux.as_mut() {
            for p in &packets {
                mux.write(p)?;
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        let tail = self.enc.flush()?;
        if let Some(mux) = self.mux.as_mut() {
            for p in &tail {
                mux.write(p)?;
            }
        }
        self.mux
            .take()
            .expect("muxer present until finish")
            .finish()
    }
}
