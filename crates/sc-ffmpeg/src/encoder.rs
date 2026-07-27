use crate::hw;
use crate::{Error, Result};
use ffmpeg_next::ffi;
use sc_core::encode::EncodedPacket;
use sc_core::time::Timestamp;
use std::ptr;

const TIMEBASE_HZ: i64 = 1_000_000; // microsecond encoder timebase for variable-rate capture

/// VA-API H.264 encoder that emits encoded packets (it does not mux). Feed it
/// NV12 frames with a monotonic nanosecond timestamp; it uploads to a GPU surface
/// and returns packets carrying real PTS, so variable-rate screen capture plays
/// back at the right speed. Global headers are always on so an MP4 muxer built
/// from this encoder gets SPS/PPS in the container header.
pub struct Encoder {
    ctx: *mut ffi::AVCodecContext,
    device: *mut ffi::AVBufferRef,
    frames: *mut ffi::AVBufferRef,
    width: i32,
    height: i32,
    base_ns: Option<i64>,
}

impl Encoder {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
        gop_frames: u32,
    ) -> Result<Self> {
        crate::init()?;
        let (w, h) = (width as i32, height as i32);
        unsafe {
            let device = hw::create_vaapi_device()?;
            let frames = hw::create_hw_frames_ctx(device, w, h)?;
            let mut enc = Self {
                ctx: ptr::null_mut(),
                device,
                frames,
                width: w,
                height: h,
                base_ns: None,
            };
            if let Err(e) = enc.open(fps as i32, bitrate_kbps as i64, gop_frames as i32) {
                enc.cleanup();
                return Err(e);
            }
            Ok(enc)
        }
    }

    pub(crate) fn ctx(&self) -> *mut ffi::AVCodecContext {
        self.ctx
    }
}

impl Encoder {
    unsafe fn open(&mut self, fps: i32, bitrate: i64, gop: i32) -> Result<()> {
        let name = std::ffi::CString::new("h264_vaapi").unwrap();
        let codec = ffi::avcodec_find_encoder_by_name(name.as_ptr());
        if codec.is_null() {
            return Err(Error::NoEncoder {
                codec: "h264_vaapi",
            });
        }
        self.ctx = ffi::avcodec_alloc_context3(codec);
        if self.ctx.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "avcodec_alloc_context3",
            });
        }
        let c = &mut *self.ctx;
        c.width = self.width;
        c.height = self.height;
        c.time_base = ffi::AVRational {
            num: 1,
            den: TIMEBASE_HZ as i32,
        };
        c.framerate = ffi::AVRational { num: fps, den: 1 };
        c.pix_fmt = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
        c.bit_rate = bitrate * 1000;
        c.gop_size = gop;
        c.max_b_frames = 0;
        c.hw_frames_ctx = ffi::av_buffer_ref(self.frames);
        c.flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
        let rc = ffi::avcodec_open2(self.ctx, codec, ptr::null_mut());
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avcodec_open2(h264_vaapi)",
            });
        }
        Ok(())
    }
}

impl Encoder {
    /// Encode one packed NV12 frame stamped with a monotonic nanosecond time.
    /// Returns any packets that became ready (usually one, sometimes zero).
    pub fn push_nv12(&mut self, nv12: &[u8], pts_nanos: i64) -> Result<Vec<EncodedPacket>> {
        let base = *self.base_ns.get_or_insert(pts_nanos);
        let pts_us = (pts_nanos - base) / (1_000_000_000 / TIMEBASE_HZ);
        unsafe { self.upload(nv12, pts_us) }
    }

    unsafe fn upload(&mut self, nv12: &[u8], pts: i64) -> Result<Vec<EncodedPacket>> {
        let (w, h) = (self.width as usize, self.height as usize);
        if nv12.len() < w * h + w * h / 2 {
            return Err(Error::Ffmpeg("short NV12 frame".into()));
        }
        let sw = ffi::av_frame_alloc();
        let hw = ffi::av_frame_alloc();
        let r = self.fill_send(sw, hw, nv12, pts);
        let (mut sw, mut hw) = (sw, hw);
        ffi::av_frame_free(&mut sw);
        ffi::av_frame_free(&mut hw);
        r
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        unsafe {
            ffi::avcodec_send_frame(self.ctx, ptr::null());
            self.drain()
        }
    }
}

impl Encoder {
    unsafe fn fill_send(
        &mut self,
        sw: *mut ffi::AVFrame,
        hw: *mut ffi::AVFrame,
        nv12: &[u8],
        pts: i64,
    ) -> Result<Vec<EncodedPacket>> {
        (*sw).format = ffi::AVPixelFormat::AV_PIX_FMT_NV12 as i32;
        (*sw).width = self.width;
        (*sw).height = self.height;
        if ffi::av_frame_get_buffer(sw, 0) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "av_frame_get_buffer",
            });
        }
        let (w, h) = (self.width as usize, self.height as usize);
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
        if ffi::av_hwframe_get_buffer(self.frames, hw, 0) < 0 {
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
        (*hw).pts = pts;
        let rc = ffi::avcodec_send_frame(self.ctx, hw);
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
        ptr::copy_nonoverlapping(src[row * w..].as_ptr(), dst.add(row * dst_stride), w);
    }
}

const AVERROR_EAGAIN: i32 = -11;

impl Encoder {
    unsafe fn drain(&mut self) -> Result<Vec<EncodedPacket>> {
        let mut out = Vec::new();
        let mut pkt = ffi::av_packet_alloc();
        loop {
            let rc = ffi::avcodec_receive_packet(self.ctx, pkt);
            if rc == AVERROR_EAGAIN || rc == ffi::AVERROR_EOF {
                break;
            }
            if rc < 0 {
                ffi::av_packet_free(&mut pkt);
                return Err(Error::Av {
                    code: rc,
                    ctx: "avcodec_receive_packet",
                });
            }
            let p = &*pkt;
            let size = p.size as usize;
            let data = std::slice::from_raw_parts(p.data, size).to_vec();
            out.push(EncodedPacket {
                timestamp: Timestamp::from_nanos(p.pts * 1000),
                duration: p.duration * 1000,
                keyframe: p.flags & ffi::AV_PKT_FLAG_KEY != 0,
                data,
            });
            ffi::av_packet_unref(pkt);
        }
        ffi::av_packet_free(&mut pkt);
        Ok(out)
    }

    fn cleanup(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                ffi::avcodec_free_context(&mut self.ctx);
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

impl Drop for Encoder {
    fn drop(&mut self) {
        self.cleanup();
    }
}
