#[cfg(target_os = "linux")]
use crate::hw;
use crate::{Error, Result};
use ffmpeg_next::ffi;
use sc_core::encode::{EncodedPacket, EncoderKind};
use sc_core::time::Timestamp;
use std::ffi::CString;
use std::ptr;

const TIMEBASE_HZ: i64 = 1_000_000; // microsecond encoder timebase for variable-rate capture

/// H.264 encoder that emits encoded packets (it does not mux). Feed it NV12
/// frames with a monotonic nanosecond timestamp and it returns packets carrying
/// real PTS, so variable-rate screen capture plays back at the right speed.
/// Global headers are always on so an MP4 muxer built from this encoder gets
/// SPS/PPS in the container header.
///
/// On Linux frames are uploaded to a VA-API surface. On Windows the hardware
/// encoders (NVENC, AMF, QSV) take system-memory NV12 directly, so no hardware
/// frame pool is needed; `new` walks the candidate list and keeps the first that
/// really opens, ending in libx264.
pub struct Encoder {
    ctx: *mut ffi::AVCodecContext,
    // Null on Windows, where frames are sent as plain NV12.
    device: *mut ffi::AVBufferRef,
    frames: *mut ffi::AVBufferRef,
    width: i32,
    height: i32,
    base_ns: Option<i64>,
    kind: EncoderKind,
    name: &'static str,
    gop: i64,
    sent: i64,
}

// Owned by, and used from, a single encode thread; moving it there is sound.
unsafe impl Send for Encoder {}

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
        let mut last_err = Error::NoEncoder { codec: "h264" };
        // `SC_ENCODER=h264_mf` (or any FFmpeg encoder name in the list) pins the
        // choice, for testing fallbacks and for working around a driver bug.
        let forced = std::env::var("SC_ENCODER").ok();
        for &(kind, name) in crate::ENCODE_ORDER {
            if forced.as_deref().is_some_and(|f| f != name) {
                continue;
            }
            let mut enc = Self {
                ctx: ptr::null_mut(),
                device: ptr::null_mut(),
                frames: ptr::null_mut(),
                width: w,
                height: h,
                base_ns: None,
                kind,
                name,
                gop: gop_frames.max(1) as i64,
                sent: 0,
            };
            match unsafe { enc.try_open(fps as i32, bitrate_kbps as i64, gop_frames as i32) } {
                Ok(()) => {
                    tracing::info!(encoder = name, "video encoder opened");
                    return Ok(enc);
                }
                Err(e) => {
                    tracing::debug!(encoder = name, error = %e, "encoder unavailable");
                    enc.cleanup();
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    /// Which encoder backend actually opened.
    pub fn kind(&self) -> EncoderKind {
        self.kind
    }

    /// FFmpeg codec name of the opened encoder (e.g. `h264_nvenc`).
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl Encoder {
    /// Platform-specific setup before the codec opens. On Linux that is the
    /// VA-API device and surface pool; VA-API is only ever tried on Linux, and
    /// other candidates there simply fail to open if the hardware is absent.
    unsafe fn try_open(&mut self, fps: i32, bitrate: i64, gop: i32) -> Result<()> {
        #[cfg(target_os = "linux")]
        if self.kind == EncoderKind::Vaapi {
            self.device = hw::create_vaapi_device()?;
            self.frames = hw::create_hw_frames_ctx(self.device, self.width, self.height)?;
        }
        self.open(fps, bitrate, gop)
    }

    unsafe fn open(&mut self, fps: i32, bitrate: i64, gop: i32) -> Result<()> {
        let cname = CString::new(self.name).unwrap();
        let codec = ffi::avcodec_find_encoder_by_name(cname.as_ptr());
        if codec.is_null() {
            return Err(Error::NoEncoder { codec: self.name });
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
        c.pix_fmt = if self.frames.is_null() {
            ffi::AVPixelFormat::AV_PIX_FMT_NV12
        } else {
            ffi::AVPixelFormat::AV_PIX_FMT_VAAPI
        };
        c.bit_rate = bitrate * 1000;
        c.gop_size = gop;
        c.max_b_frames = 0;
        if !self.frames.is_null() {
            c.hw_frames_ctx = ffi::av_buffer_ref(self.frames);
        }
        c.flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
        // Constrain the rate so the ring buffer's byte budget (sized from the
        // configured bitrate) actually holds N seconds.
        #[cfg(windows)]
        {
            c.rc_max_rate = bitrate * 1000;
            c.rc_buffer_size = (bitrate * 1000) as i32;
            // Tag the stream BT.709 / limited range to match the converter, so
            // players don't guess (and shift colours).
            c.colorspace = ffi::AVColorSpace::AVCOL_SPC_BT709;
            c.color_primaries = ffi::AVColorPrimaries::AVCOL_PRI_BT709;
            c.color_trc = ffi::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            c.color_range = ffi::AVColorRange::AVCOL_RANGE_MPEG;
        }
        let mut opts = ptr::null_mut();
        for (k, v) in encoder_options(self.kind, self.name) {
            let (k, v) = (CString::new(*k).unwrap(), CString::new(*v).unwrap());
            ffi::av_dict_set(&mut opts, k.as_ptr(), v.as_ptr(), 0);
        }
        let rc = ffi::avcodec_open2(self.ctx, codec, &mut opts);
        ffi::av_dict_free(&mut opts);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avcodec_open2",
            });
        }
        Ok(())
    }
}

/// Low-latency, constant-ish-rate settings per encoder. Replay capture wants the
/// encoder to keep up in real time and never stall the pipeline.
fn encoder_options(kind: EncoderKind, name: &str) -> &'static [(&'static str, &'static str)] {
    match kind {
        EncoderKind::Nvenc => &[
            ("preset", "p4"),
            ("tune", "ll"),
            ("rc", "cbr"),
            ("zerolatency", "1"),
            ("forced-idr", "1"),
        ],
        EncoderKind::Amf => &[
            ("usage", "lowlatency"),
            ("quality", "balanced"),
            ("rc", "cbr"),
            ("forced_idr", "1"),
        ],
        EncoderKind::Qsv => &[("preset", "veryfast"), ("look_ahead", "0")],
        EncoderKind::Software if name == "libx264" => {
            &[("preset", "veryfast"), ("tune", "zerolatency")]
        }
        EncoderKind::Software => &[],
        EncoderKind::Vaapi => &[],
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
        if self.frames.is_null() {
            // Software-memory path (Windows): the encoder takes NV12 directly.
            // Several hardware encoders (AMF in particular) ignore gop_size, which
            // would leave one endless GOP the ring buffer can never trim, so ask
            // for an IDR explicitly every `gop` frames.
            if self.sent % self.gop == 0 {
                (*sw).pict_type = ffi::AVPictureType::AV_PICTURE_TYPE_I;
            }
            self.sent += 1;
            (*sw).pts = pts;
            let rc = ffi::avcodec_send_frame(self.ctx, sw);
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "avcodec_send_frame",
                });
            }
            return self.drain();
        }
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

/// An owned snapshot of the encoder's codec parameters (dimensions + SPS/PPS
/// extradata). Lets a muxer be built on a save thread without touching the live
/// encoder. Safe to send across threads: it is plain owned data with no aliasing.
pub struct CodecParams(*mut ffi::AVCodecParameters);

// Send + Sync: after creation the AVCodecParameters is immutable; the muxer only
// reads it (avcodec_parameters_copy), so sharing a reference across threads is safe.
unsafe impl Send for CodecParams {}
unsafe impl Sync for CodecParams {}

impl CodecParams {
    pub(crate) fn as_ptr(&self) -> *const ffi::AVCodecParameters {
        self.0
    }

    /// Snapshot the codec parameters (dimensions/format + extradata) of any open
    /// encoder context.
    pub(crate) fn from_context(ctx: *mut ffi::AVCodecContext) -> Result<Self> {
        unsafe {
            let p = ffi::avcodec_parameters_alloc();
            if p.is_null() {
                return Err(Error::Av {
                    code: -1,
                    ctx: "avcodec_parameters_alloc",
                });
            }
            let params = CodecParams(p);
            if ffi::avcodec_parameters_from_context(params.0, ctx) < 0 {
                return Err(Error::Av {
                    code: -1,
                    ctx: "avcodec_parameters_from_context",
                });
            }
            Ok(params)
        }
    }
}

impl Drop for CodecParams {
    fn drop(&mut self) {
        unsafe { ffi::avcodec_parameters_free(&mut self.0) };
    }
}

impl Encoder {
    pub fn codec_params(&self) -> Result<CodecParams> {
        CodecParams::from_context(self.ctx)
    }
}
