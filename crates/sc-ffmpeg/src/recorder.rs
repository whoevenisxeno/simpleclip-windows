use crate::hw;
use crate::{Error, Result};
use ffmpeg_next::ffi;
use std::ffi::CString;
use std::ptr;

// AVERROR(EAGAIN) on Linux; this crate only compiles on Linux (see lib.rs).
const AVERROR_EAGAIN: i32 = -11;

/// A live H.264/VA-API encode + MP4 mux session. Feed it NV12 frames; it uploads
/// each to a GPU surface, encodes on the AMD/Intel VA-API block, and interleaves
/// the packets into a faststart MP4. Not Send: the FFmpeg contexts are used from
/// one thread (the daemon's encode thread).
pub struct Recorder {
    oc: *mut ffi::AVFormatContext,
    enc: *mut ffi::AVCodecContext,
    device: *mut ffi::AVBufferRef,
    frames: *mut ffi::AVBufferRef,
    width: i32,
    height: i32,
    fps: i32,
    next_pts: i64,
    stream_tb: ffi::AVRational,
    finished: bool,
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
        crate::init()?;
        let (w, h, fps) = (width as i32, height as i32, fps as i32);
        let path_c = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| Error::Ffmpeg("path has interior NUL".into()))?;
        unsafe {
            let device = hw::create_vaapi_device()?;
            let frames = hw::create_hw_frames_ctx(device, w, h)?;
            let mut rec = Self {
                oc: ptr::null_mut(),
                enc: ptr::null_mut(),
                device,
                frames,
                width: w,
                height: h,
                fps,
                next_pts: 0,
                stream_tb: ffi::AVRational { num: 1, den: fps },
                finished: false,
            };
            if let Err(e) = rec.open(&path_c, bitrate_kbps as i64, gop_frames as i32) {
                rec.cleanup();
                return Err(e);
            }
            Ok(rec)
        }
    }
}

impl Recorder {
    /// # Safety: called once from `new`, before any frame is pushed.
    unsafe fn open(&mut self, path_c: &CString, bitrate: i64, gop: i32) -> Result<()> {
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

        let name = CString::new("h264_vaapi").unwrap();
        let codec = ffi::avcodec_find_encoder_by_name(name.as_ptr());
        if codec.is_null() {
            return Err(Error::NoEncoder {
                codec: "h264_vaapi",
            });
        }
        self.enc = ffi::avcodec_alloc_context3(codec);
        if self.enc.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "avcodec_alloc_context3",
            });
        }
        let enc = &mut *self.enc;
        enc.width = self.width;
        enc.height = self.height;
        enc.time_base = ffi::AVRational {
            num: 1,
            den: self.fps,
        };
        enc.framerate = ffi::AVRational {
            num: self.fps,
            den: 1,
        };
        enc.pix_fmt = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
        enc.bit_rate = bitrate * 1000;
        enc.gop_size = gop;
        enc.max_b_frames = 0;
        enc.hw_frames_ctx = ffi::av_buffer_ref(self.frames);
        if (*(*self.oc).oformat).flags & ffi::AVFMT_GLOBALHEADER != 0 {
            enc.flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
        }
        self.open_stream(codec, path_c)
    }
}

impl Recorder {
    unsafe fn open_stream(&mut self, codec: *const ffi::AVCodec, path_c: &CString) -> Result<()> {
        let rc = ffi::avcodec_open2(self.enc, codec, ptr::null_mut());
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avcodec_open2(h264_vaapi)",
            });
        }
        let st = ffi::avformat_new_stream(self.oc, ptr::null());
        if st.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "avformat_new_stream",
            });
        }
        (*st).time_base = (*self.enc).time_base;
        let rc = ffi::avcodec_parameters_from_context((*st).codecpar, self.enc);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avcodec_parameters_from_context",
            });
        }
        let rc = ffi::avio_open(&mut (*self.oc).pb, path_c.as_ptr(), ffi::AVIO_FLAG_WRITE);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
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

impl Recorder {
    /// Encode one tightly-packed NV12 frame (Y plane of w*h, then interleaved UV
    /// of w*h/2). Uploads to a GPU surface and hands it to the VA-API encoder.
    pub fn push_nv12(&mut self, nv12: &[u8]) -> Result<()> {
        let expected = (self.width * self.height + self.width * self.height / 2) as usize;
        if nv12.len() < expected {
            return Err(Error::Ffmpeg(format!(
                "short NV12 frame: {} < {expected}",
                nv12.len()
            )));
        }
        unsafe { self.upload_and_send(nv12) }
    }

    unsafe fn upload_and_send(&mut self, nv12: &[u8]) -> Result<()> {
        let sw = ffi::av_frame_alloc();
        let hw = ffi::av_frame_alloc();
        let result = self.fill_upload_encode(sw, hw, nv12);
        let mut sw = sw;
        let mut hw = hw;
        ffi::av_frame_free(&mut sw);
        ffi::av_frame_free(&mut hw);
        result
    }
}

impl Recorder {
    unsafe fn fill_upload_encode(
        &mut self,
        sw: *mut ffi::AVFrame,
        hw: *mut ffi::AVFrame,
        nv12: &[u8],
    ) -> Result<()> {
        (*sw).format = ffi::AVPixelFormat::AV_PIX_FMT_NV12 as i32;
        (*sw).width = self.width;
        (*sw).height = self.height;
        if ffi::av_frame_get_buffer(sw, 0) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "av_frame_get_buffer(sw)",
            });
        }
        let (w, h) = (self.width as usize, self.height as usize);
        // Y plane, then interleaved UV plane, honoring FFmpeg's linesize padding.
        copy_plane(
            (*sw).data[0],
            (*sw).linesize[0] as usize,
            &nv12[..w * h],
            w,
            h,
        );
        copy_plane(
            (*sw).data[1],
            (*sw).linesize[1] as usize,
            &nv12[w * h..],
            w,
            h / 2,
        );

        if ffi::av_hwframe_get_buffer(self.frames, hw, 0) < 0 || (*hw).hw_frames_ctx.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "av_hwframe_get_buffer",
            });
        }
        if ffi::av_hwframe_transfer_data(hw, sw, 0) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "av_hwframe_transfer_data",
            });
        }
        (*hw).pts = self.next_pts;
        self.next_pts += 1;
        let rc = ffi::avcodec_send_frame(self.enc, hw);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avcodec_send_frame",
            });
        }
        self.drain()
    }
}

unsafe fn copy_plane(dst: *mut u8, dst_stride: usize, src: &[u8], w: usize, rows: usize) {
    for row in 0..rows {
        let s = &src[row * w..row * w + w];
        ptr::copy_nonoverlapping(s.as_ptr(), dst.add(row * dst_stride), w);
    }
}

impl Recorder {
    /// Pull every ready packet from the encoder and interleave it into the MP4.
    unsafe fn drain(&mut self) -> Result<()> {
        let mut pkt = ffi::av_packet_alloc();
        let result = self.drain_loop(pkt);
        ffi::av_packet_free(&mut pkt);
        result
    }

    unsafe fn drain_loop(&mut self, pkt: *mut ffi::AVPacket) -> Result<()> {
        loop {
            let rc = ffi::avcodec_receive_packet(self.enc, pkt);
            if rc == AVERROR_EAGAIN || rc == ffi::AVERROR_EOF {
                return Ok(());
            }
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "avcodec_receive_packet",
                });
            }
            ffi::av_packet_rescale_ts(pkt, (*self.enc).time_base, self.stream_tb);
            (*pkt).stream_index = 0;
            let rc = ffi::av_interleaved_write_frame(self.oc, pkt);
            ffi::av_packet_unref(pkt);
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "av_interleaved_write_frame",
                });
            }
        }
    }

    /// Flush the encoder, write the MP4 trailer (finalizes the moov atom /
    /// faststart), and release everything.
    pub fn finish(mut self) -> Result<()> {
        unsafe {
            ffi::avcodec_send_frame(self.enc, ptr::null());
            self.drain()?;
            let rc = ffi::av_write_trailer(self.oc);
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "av_write_trailer",
                });
            }
        }
        self.finished = true;
        Ok(())
    }

    /// Free every FFmpeg context. Idempotent via null-checks; runs from both
    /// `finish` (via Drop) and the error path in `new`.
    fn cleanup(&mut self) {
        unsafe {
            if !self.enc.is_null() {
                ffi::avcodec_free_context(&mut self.enc);
            }
            if !self.oc.is_null() {
                if !(*self.oc).pb.is_null() {
                    ffi::avio_closep(&mut (*self.oc).pb);
                }
                ffi::avformat_free_context(self.oc);
                self.oc = ptr::null_mut();
            }
            if !self.frames.is_null() {
                ffi::av_buffer_unref(&mut self.frames);
            }
            if !self.device.is_null() {
                ffi::av_buffer_unref(&mut self.device);
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.finished && !self.oc.is_null() {
            tracing::warn!("Recorder dropped without finish(); MP4 trailer not written");
        }
        self.cleanup();
    }
}
