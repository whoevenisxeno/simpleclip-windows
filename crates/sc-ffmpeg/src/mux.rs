use crate::encoder::{CodecParams, Encoder};
use crate::{Error, Result};
use ffmpeg_next::ffi;
use sc_core::encode::EncodedPacket;
use std::ffi::CString;
use std::ptr;

// Packet timestamps arrive in nanoseconds; we carry them through the muxer as
// microseconds, then rescale to each stream's own timebase.
const IN_TB: ffi::AVRational = ffi::AVRational {
    num: 1,
    den: 1_000_000,
};

/// Muxes encoded video (and optionally AAC audio) into a faststart MP4. Built
/// from encoder param snapshots so it can run on a save thread. Packet
/// timestamps are normalized to the first packet written across both streams, so
/// a clip taken out of the ring buffer starts at zero and stays A/V-aligned.
pub struct Mp4Muxer {
    oc: *mut ffi::AVFormatContext,
    video_tb: ffi::AVRational,
    audio: Option<(i32, ffi::AVRational)>,
    base_ns: Option<i64>,
    finished: bool,
}

impl Mp4Muxer {
    pub fn new(path: &std::path::Path, enc: &Encoder) -> Result<Self> {
        Self::from_params(path, &enc.codec_params()?)
    }

    pub fn from_params(path: &std::path::Path, video: &CodecParams) -> Result<Self> {
        Self::open_all(path, video, None)
    }

    /// Video plus an AAC audio stream at `audio_rate` Hz.
    pub fn with_audio(
        path: &std::path::Path,
        video: &CodecParams,
        audio: &CodecParams,
        audio_rate: i32,
    ) -> Result<Self> {
        Self::open_all(path, video, Some((audio, audio_rate)))
    }
}

impl Mp4Muxer {
    fn open_all(
        path: &std::path::Path,
        video: &CodecParams,
        audio: Option<(&CodecParams, i32)>,
    ) -> Result<Self> {
        let path_c = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| Error::Ffmpeg("path has interior NUL".into()))?;
        unsafe {
            let mut m = Self {
                oc: ptr::null_mut(),
                video_tb: IN_TB,
                audio: None,
                base_ns: None,
                finished: false,
            };
            if let Err(e) = m.open(&path_c, video, audio) {
                m.cleanup();
                return Err(e);
            }
            Ok(m)
        }
    }

    unsafe fn add_stream(
        &mut self,
        params: *const ffi::AVCodecParameters,
        tb: ffi::AVRational,
    ) -> Result<i32> {
        let st = ffi::avformat_new_stream(self.oc, ptr::null());
        if st.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "avformat_new_stream",
            });
        }
        if ffi::avcodec_parameters_copy((*st).codecpar, params) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "avcodec_parameters_copy",
            });
        }
        (*st).time_base = tb;
        Ok((*st).index)
    }
}

impl Mp4Muxer {
    unsafe fn open(
        &mut self,
        path_c: &CString,
        video: &CodecParams,
        audio: Option<(&CodecParams, i32)>,
    ) -> Result<()> {
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
        let vidx = self.add_stream(video.as_ptr(), IN_TB)?;
        let aidx = match audio {
            Some((p, rate)) => Some((
                self.add_stream(p.as_ptr(), ffi::AVRational { num: 1, den: rate })?,
                rate,
            )),
            None => None,
        };
        if ffi::avio_open(&mut (*self.oc).pb, path_c.as_ptr(), ffi::AVIO_FLAG_WRITE) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "avio_open",
            });
        }
        let mut opts: *mut ffi::AVDictionary = ptr::null_mut();
        let (k, v) = (
            CString::new("movflags").unwrap(),
            CString::new("+faststart").unwrap(),
        );
        ffi::av_dict_set(&mut opts, k.as_ptr(), v.as_ptr(), 0);
        let rc = ffi::avformat_write_header(self.oc, &mut opts);
        ffi::av_dict_free(&mut opts);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_write_header",
            });
        }
        self.video_tb = stream_tb(self.oc, vidx);
        if let Some((ai, _)) = aidx {
            self.audio = Some((ai, stream_tb(self.oc, ai)));
        }
        Ok(())
    }
}

unsafe fn stream_tb(oc: *mut ffi::AVFormatContext, idx: i32) -> ffi::AVRational {
    (*(*(*oc).streams.add(idx as usize))).time_base
}

impl Mp4Muxer {
    /// Write a video packet (stream 0).
    pub fn write(&mut self, ep: &EncodedPacket) -> Result<()> {
        let tb = self.video_tb;
        self.write_pkt(ep, 0, tb)
    }

    /// Write an audio packet, if this muxer has an audio stream.
    pub fn write_audio(&mut self, ep: &EncodedPacket) -> Result<()> {
        if let Some((idx, tb)) = self.audio {
            return self.write_pkt(ep, idx, tb);
        }
        Ok(())
    }

    fn write_pkt(&mut self, ep: &EncodedPacket, idx: i32, tb: ffi::AVRational) -> Result<()> {
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
            let ts = ffi::av_rescale_q(pts_us.max(0), IN_TB, tb);
            (*pkt).pts = ts;
            (*pkt).dts = ts;
            (*pkt).stream_index = idx;
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
