use crate::encoder::Encoder;
use crate::{Error, Result};
use ffmpeg_next::ffi;
use sc_core::encode::EncodedPacket;
use std::ffi::CString;
use std::ptr;

const IN_TB: ffi::AVRational = ffi::AVRational {
    num: 1,
    den: 1_000_000,
};

/// Muxes already-encoded packets into a faststart MP4. Built from an `Encoder`
/// so it copies the right codec parameters (SPS/PPS). Packet timestamps are
/// normalized to the first packet written, so a clip snapshotted out of the ring
/// buffer starts cleanly at zero.
pub struct Mp4Muxer {
    oc: *mut ffi::AVFormatContext,
    stream_tb: ffi::AVRational,
    base_ns: Option<i64>,
    finished: bool,
}

impl Mp4Muxer {
    pub fn new(path: &std::path::Path, enc: &Encoder) -> Result<Self> {
        let path_c = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| Error::Ffmpeg("path has interior NUL".into()))?;
        unsafe {
            let mut m = Self {
                oc: ptr::null_mut(),
                stream_tb: IN_TB,
                base_ns: None,
                finished: false,
            };
            if let Err(e) = m.open(&path_c, enc) {
                m.cleanup();
                return Err(e);
            }
            Ok(m)
        }
    }
}

impl Mp4Muxer {
    unsafe fn open(&mut self, path_c: &CString, enc: &Encoder) -> Result<()> {
        let mp4 = CString::new("mp4").unwrap();
        let rc = ffi::avformat_alloc_output_context2(
            &mut self.oc,
            ptr::null_mut(),
            mp4.as_ptr(),
            path_c.as_ptr(),
        );
        if rc < 0 || self.oc.is_null() {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_alloc_output_context2",
            });
        }
        let st = ffi::avformat_new_stream(self.oc, ptr::null());
        if st.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "avformat_new_stream",
            });
        }
        if ffi::avcodec_parameters_from_context((*st).codecpar, enc.ctx()) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "avcodec_parameters_from_context",
            });
        }
        (*st).time_base = IN_TB;
        if ffi::avio_open(&mut (*self.oc).pb, path_c.as_ptr(), ffi::AVIO_FLAG_WRITE) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "avio_open",
            });
        }
        let mut opts: *mut ffi::AVDictionary = ptr::null_mut();
        let k = CString::new("movflags").unwrap();
        let v = CString::new("+faststart").unwrap();
        ffi::av_dict_set(&mut opts, k.as_ptr(), v.as_ptr(), 0);
        let rc = ffi::avformat_write_header(self.oc, &mut opts);
        ffi::av_dict_free(&mut opts);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_write_header",
            });
        }
        self.stream_tb = (*st).time_base;
        Ok(())
    }
}

impl Mp4Muxer {
    /// Write one encoded packet. Timestamps are normalized to the first packet
    /// so the output starts at zero regardless of when it was captured.
    pub fn write(&mut self, ep: &EncodedPacket) -> Result<()> {
        let base = *self.base_ns.get_or_insert(ep.timestamp.as_nanos());
        let pts_us = (ep.timestamp.as_nanos() - base) / 1000;
        unsafe {
            let mut pkt = ffi::av_packet_alloc();
            if ffi::av_new_packet(pkt, ep.data.len() as i32) < 0 {
                ffi::av_packet_free(&mut pkt);
                return Err(Error::Av {
                    code: -1,
                    ctx: "av_new_packet",
                });
            }
            ptr::copy_nonoverlapping(ep.data.as_ptr(), (*pkt).data, ep.data.len());
            let ts = ffi::av_rescale_q(pts_us, IN_TB, self.stream_tb);
            (*pkt).pts = ts;
            (*pkt).dts = ts;
            (*pkt).stream_index = 0;
            if ep.keyframe {
                (*pkt).flags |= ffi::AV_PKT_FLAG_KEY;
            }
            let rc = ffi::av_interleaved_write_frame(self.oc, pkt);
            ffi::av_packet_free(&mut pkt);
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "av_interleaved_write_frame",
                });
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        unsafe {
            if ffi::av_write_trailer(self.oc) < 0 {
                return Err(Error::Av {
                    code: -1,
                    ctx: "av_write_trailer",
                });
            }
        }
        self.finished = true;
        Ok(())
    }

    fn cleanup(&mut self) {
        unsafe {
            if !self.oc.is_null() {
                if !(*self.oc).pb.is_null() {
                    ffi::avio_closep(&mut (*self.oc).pb);
                }
                ffi::avformat_free_context(self.oc);
                self.oc = ptr::null_mut();
            }
        }
    }
}

impl Drop for Mp4Muxer {
    fn drop(&mut self) {
        if !self.finished && !self.oc.is_null() {
            tracing::warn!("Mp4Muxer dropped without finish(); file incomplete");
        }
        self.cleanup();
    }
}
