use crate::encoder::CodecParams;
use crate::{Error, Result};
use ffmpeg_next::ffi;
use sc_core::audio::AudioBuffer;
use sc_core::encode::EncodedPacket;
use sc_core::time::Timestamp;
use std::ptr;

/// AAC encoder for desktop audio. Buffers interleaved f32 into AAC-sized frames,
/// deinterleaves to planar float, and emits packets timestamped on the shared
/// capture clock so they mux in sync with video.
pub struct AacEncoder {
    ctx: *mut ffi::AVCodecContext,
    rate: i32,
    channels: i32,
    frame_size: usize,
    pending: Vec<f32>,
    base_ns: Option<i64>,
    samples_done: i64,
}

unsafe impl Send for AacEncoder {}

impl AacEncoder {
    pub fn new(sample_rate: u32, channels: u16, bitrate_kbps: u32) -> Result<Self> {
        crate::init()?;
        let (rate, ch) = (sample_rate as i32, channels.max(1) as i32);
        unsafe {
            let mut e = Self {
                ctx: ptr::null_mut(),
                rate,
                channels: ch,
                frame_size: 0,
                pending: Vec::new(),
                base_ns: None,
                samples_done: 0,
            };
            if let Err(err) = e.open(bitrate_kbps as i64) {
                e.cleanup();
                return Err(err);
            }
            Ok(e)
        }
    }

    pub fn codec_params(&self) -> Result<CodecParams> {
        CodecParams::from_context(self.ctx)
    }

    pub fn rate(&self) -> i32 {
        self.rate
    }
}

impl AacEncoder {
    unsafe fn open(&mut self, bitrate: i64) -> Result<()> {
        let name = std::ffi::CString::new("aac").unwrap();
        let codec = ffi::avcodec_find_encoder_by_name(name.as_ptr());
        if codec.is_null() {
            return Err(Error::NoEncoder { codec: "aac" });
        }
        self.ctx = ffi::avcodec_alloc_context3(codec);
        if self.ctx.is_null() {
            return Err(Error::Av {
                code: -1,
                ctx: "avcodec_alloc_context3(aac)",
            });
        }
        (*self.ctx).sample_rate = self.rate;
        (*self.ctx).sample_fmt = ffi::AVSampleFormat::AV_SAMPLE_FMT_FLTP;
        (*self.ctx).bit_rate = bitrate * 1000;
        ffi::av_channel_layout_default(&mut (*self.ctx).ch_layout, self.channels);
        (*self.ctx).time_base = ffi::AVRational {
            num: 1,
            den: self.rate,
        };
        (*self.ctx).flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
        if ffi::avcodec_open2(self.ctx, codec, ptr::null_mut()) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "avcodec_open2(aac)",
            });
        }
        let fs = (*self.ctx).frame_size;
        self.frame_size = if fs > 0 { fs as usize } else { 1024 };
        Ok(())
    }

    pub fn push(&mut self, buf: &AudioBuffer) -> Result<Vec<EncodedPacket>> {
        self.base_ns.get_or_insert(buf.timestamp.as_nanos());
        self.pending.extend_from_slice(&buf.samples);
        let chunk = self.frame_size * self.channels as usize;
        let mut out = Vec::new();
        while self.pending.len() >= chunk {
            let frame: Vec<f32> = self.pending.drain(..chunk).collect();
            out.extend(unsafe { self.encode_frame(&frame)? });
        }
        Ok(out)
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedPacket>> {
        unsafe {
            ffi::avcodec_send_frame(self.ctx, ptr::null());
            self.drain()
        }
    }
}

const AVERROR_EAGAIN: i32 = -11;

impl AacEncoder {
    unsafe fn encode_frame(&mut self, interleaved: &[f32]) -> Result<Vec<EncodedPacket>> {
        let frame = ffi::av_frame_alloc();
        (*frame).nb_samples = self.frame_size as i32;
        (*frame).format = ffi::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
        ffi::av_channel_layout_copy(&mut (*frame).ch_layout, &(*self.ctx).ch_layout);
        (*frame).sample_rate = self.rate;
        if ffi::av_frame_get_buffer(frame, 0) < 0 {
            let mut f = frame;
            ffi::av_frame_free(&mut f);
            return Err(Error::Av {
                code: -1,
                ctx: "av_frame_get_buffer(audio)",
            });
        }
        let ch = self.channels as usize;
        for c in 0..ch {
            let plane = (*frame).data[c] as *mut f32;
            for i in 0..self.frame_size {
                *plane.add(i) = interleaved[i * ch + c];
            }
        }
        (*frame).pts = self.samples_done;
        let rc = ffi::avcodec_send_frame(self.ctx, frame);
        let mut f = frame;
        ffi::av_frame_free(&mut f);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avcodec_send_frame(audio)",
            });
        }
        self.samples_done += self.frame_size as i64;
        self.drain()
    }

    unsafe fn drain(&mut self) -> Result<Vec<EncodedPacket>> {
        let base = self.base_ns.unwrap_or(0);
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
                    ctx: "avcodec_receive_packet(audio)",
                });
            }
            let p = &*pkt;
            let ns = base + p.pts * 1_000_000_000 / self.rate as i64;
            out.push(EncodedPacket {
                timestamp: Timestamp::from_nanos(ns),
                duration: p.duration * 1_000_000_000 / self.rate as i64,
                keyframe: true,
                data: std::slice::from_raw_parts(p.data, p.size as usize).to_vec(),
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
        }
    }
}

impl Drop for AacEncoder {
    fn drop(&mut self) {
        self.cleanup();
    }
}
