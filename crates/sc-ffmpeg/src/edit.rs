use crate::compress::{decode_audio_mix, source_fps};
use crate::stretch::stretch;
use crate::{AacEncoder, Encoder, Error, Mp4Muxer, Result};
use ffmpeg_next as ff;
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::edit::{orient, Overlay, Plan};
use sc_core::time::Timestamp;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const AUDIO_RATE: u32 = 48_000;
const AUDIO_KBPS: u32 = 128;
const JOIN_FADE: usize = 288;
const MIN_KBPS: f64 = 1_500.0;
const MAX_KBPS: f64 = 60_000.0;

pub const CANCELLED: &str = "cancelled";

pub struct Canvas<'a> {
    pub width: usize,
    pub height: usize,
    pub nv12: &'a mut [u8],
}

pub type Painter<'a> = dyn FnMut(f64, &mut Canvas) + 'a;

#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    pub width: usize,
    pub height: usize,
    luma: Vec<u8>,
    alpha: Vec<u8>,
    chroma: Vec<[u8; 3]>,
}

fn to_yuv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    (
        16.0 + 219.0 * y / 255.0,
        128.0 + 224.0 * (b - y) / (1.8556 * 255.0),
        128.0 + 224.0 * (r - y) / (1.5748 * 255.0),
    )
}

fn mix(over: u8, under: u8, a: u32) -> u8 {
    ((over as u32 * a + under as u32 * (255 - a) + 127) / 255) as u8
}

impl Stamp {
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Self {
        let n = width * height;
        let mut luma = Vec::with_capacity(n);
        let mut alpha = Vec::with_capacity(n);
        for p in rgba[..n * 4].as_chunks::<4>().0 {
            luma.push(to_yuv(p[0] as f32, p[1] as f32, p[2] as f32).0.round() as u8);
            alpha.push(p[3]);
        }
        let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
        let mut chroma = Vec::with_capacity(cw * ch);
        for cy in 0..ch {
            for cx in 0..cw {
                let (mut u, mut v, mut a, mut k) = (0.0, 0.0, 0.0, 0.0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (x, y) = (cx * 2 + dx, cy * 2 + dy);
                    if x >= width || y >= height {
                        continue;
                    }
                    let p = &rgba[(y * width + x) * 4..][..4];
                    let w = p[3] as f32;
                    let (_, pu, pv) = to_yuv(p[0] as f32, p[1] as f32, p[2] as f32);
                    u += pu * w;
                    v += pv * w;
                    a += w;
                    k += 1.0;
                }
                chroma.push(if a > 0.0 {
                    [
                        (u / a).round() as u8,
                        (v / a).round() as u8,
                        (a / k).round() as u8,
                    ]
                } else {
                    [128, 128, 0]
                });
            }
        }
        Stamp {
            width,
            height,
            luma,
            alpha,
            chroma,
        }
    }
}

impl Canvas<'_> {
    pub fn stamp(&mut self, x0: i64, y0: i64, s: &Stamp) {
        let (w, h) = (self.width as i64, self.height as i64);
        let x0 = x0 & !1;
        let y0 = y0 & !1;
        let (plane, uv) = self.nv12.split_at_mut(self.width * self.height);
        for row in 0..s.height as i64 {
            let y = y0 + row;
            if y < 0 || y >= h {
                continue;
            }
            for col in 0..s.width as i64 {
                let x = x0 + col;
                if x < 0 || x >= w {
                    continue;
                }
                let i = (row * s.width as i64 + col) as usize;
                let a = s.alpha[i] as u32;
                if a == 0 {
                    continue;
                }
                let d = &mut plane[(y * w + x) as usize];
                *d = mix(s.luma[i], *d, a);
            }
        }
        let cw = s.width.div_ceil(2) as i64;
        for crow in 0..s.height.div_ceil(2) as i64 {
            let y = y0 / 2 + crow;
            if y < 0 || y >= h / 2 {
                continue;
            }
            for ccol in 0..cw {
                let x = x0 / 2 + ccol;
                if x < 0 || x >= w / 2 {
                    continue;
                }
                let [u, v, a] = s.chroma[(crow * cw + ccol) as usize];
                if a == 0 {
                    continue;
                }
                let at = ((y * (w / 2) + x) * 2) as usize;
                uv[at] = mix(u, uv[at], a as u32);
                uv[at + 1] = mix(v, uv[at + 1], a as u32);
            }
        }
    }
}

#[derive(Default)]
pub struct Progress {
    permille: AtomicU32,
    cancel: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        self.permille.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn set(&self, f: f64) {
        self.permille
            .store((f.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct Rendered {
    pub secs: f64,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

fn err(e: ff::Error) -> Error {
    Error::Ffmpeg(e.to_string())
}

fn shifted(mix: &[f32], secs: f64) -> std::borrow::Cow<'_, [f32]> {
    let frames = (secs.abs() * AUDIO_RATE as f64).round() as usize;
    if frames == 0 {
        return std::borrow::Cow::Borrowed(mix);
    }
    if secs > 0.0 {
        let mut out = vec![0.0f32; frames * 2];
        out.extend_from_slice(mix);
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(&mix[(frames * 2).min(mix.len())..])
    }
}

fn edit_audio(mix: &[f32], plan: &Plan) -> Vec<f32> {
    let mix = shifted(mix, plan.audio_shift);
    let mix = &mix[..];
    let mut out: Vec<f32> = Vec::new();
    let last = plan.segments.len().saturating_sub(1);
    for (i, seg) in plan.segments.iter().enumerate() {
        let a = ((seg.start * AUDIO_RATE as f64) as usize * 2).min(mix.len()) & !1;
        let b = ((seg.end * AUDIO_RATE as f64) as usize * 2).min(mix.len()) & !1;
        let mut piece = mix[a..b.max(a)].to_vec();
        let frames = piece.len() / 2;
        let ramp = JOIN_FADE.min(frames / 2);
        for f in 0..ramp {
            let g = f as f32 / ramp as f32;
            if i > 0 {
                piece[f * 2] *= g;
                piece[f * 2 + 1] *= g;
            }
            if i < last {
                let r = frames - 1 - f;
                piece[r * 2] *= g;
                piece[r * 2 + 1] *= g;
            }
        }
        out.extend_from_slice(&stretch(&piece, seg.speed));
    }
    let total = out.len() / 2;
    let secs = total as f64 / AUDIO_RATE as f64;
    let gain = plan.volume;
    let fading = plan.fade_in > 0.0 || plan.fade_out > 0.0;
    for (f, pair) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let mut g = gain;
        if fading {
            g *= plan.fade_gain(f as f64 / AUDIO_RATE as f64, secs);
        }
        pair[0] = (pair[0] * g).clamp(-1.0, 1.0);
        pair[1] = (pair[1] * g).clamp(-1.0, 1.0);
    }
    out
}

struct Sink {
    enc: Encoder,
    mux: Mp4Muxer,
    aac: Option<AacEncoder>,
    audio: Vec<f32>,
    audio_pos: usize,
    last_ns: i64,
}

impl Sink {
    fn push(&mut self, nv12: &[u8], out_t: f64) -> Result<()> {
        let ns = ((out_t.max(0.0)) * 1e9) as i64;
        let ns = ns.max(self.last_ns + 1);
        self.last_ns = ns;
        for p in self.enc.push_nv12(nv12, ns)? {
            self.mux.write(&p)?;
        }
        self.feed(ns as f64 / 1e9 + 0.1)
    }

    fn feed(&mut self, until: f64) -> Result<()> {
        let Some(aac) = self.aac.as_mut() else {
            return Ok(());
        };
        let chunk = 1024 * 2;
        while self.audio_pos < self.audio.len() {
            let t = (self.audio_pos / 2) as f64 / AUDIO_RATE as f64;
            if t > until {
                break;
            }
            let end = (self.audio_pos + chunk).min(self.audio.len());
            let buf = AudioBuffer {
                role: AudioRole::Desktop,
                sample_rate: AUDIO_RATE,
                channels: 2,
                timestamp: Timestamp::from_nanos((t * 1e9) as i64),
                samples: self.audio[self.audio_pos..end].to_vec(),
            };
            self.audio_pos = end;
            for p in aac.push(&buf)? {
                self.mux.write_audio(&p)?;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        self.feed(f64::MAX)?;
        for p in self.enc.flush()? {
            self.mux.write(&p)?;
        }
        if let Some(aac) = self.aac.as_mut() {
            for p in aac.flush()? {
                self.mux.write_audio(&p)?;
            }
        }
        self.mux.finish()
    }
}

struct Converter {
    to_rgba: Option<ff::software::scaling::Context>,
    to_nv12: Option<ff::software::scaling::Context>,
    rgba: ff::frame::Video,
    staged: Option<ff::frame::Video>,
    nv12: ff::frame::Video,
    out: (u32, u32),
}

impl Converter {
    fn new(out: (u32, u32)) -> Self {
        Self {
            to_rgba: None,
            to_nv12: None,
            rgba: ff::frame::Video::empty(),
            staged: None,
            nv12: ff::frame::Video::empty(),
            out,
        }
    }

    fn plain(&mut self, frame: &ff::frame::Video) -> Result<Vec<u8>> {
        if self.to_nv12.is_none() {
            self.to_nv12 = Some(
                ff::software::scaling::Context::get(
                    frame.format(),
                    frame.width(),
                    frame.height(),
                    ff::format::Pixel::NV12,
                    self.out.0,
                    self.out.1,
                    ff::software::scaling::Flags::BICUBIC,
                )
                .map_err(err)?,
            );
        }
        self.to_nv12
            .as_mut()
            .unwrap()
            .run(frame, &mut self.nv12)
            .map_err(err)?;
        Ok(pack_nv12(&self.nv12, self.out))
    }

    fn styled(&mut self, frame: &ff::frame::Video, plan: &Plan, gain: f32) -> Result<Vec<u8>> {
        let (sw, sh) = (frame.width(), frame.height());
        if self.to_rgba.is_none() {
            self.to_rgba = Some(
                ff::software::scaling::Context::get(
                    frame.format(),
                    sw,
                    sh,
                    ff::format::Pixel::RGBA,
                    sw,
                    sh,
                    ff::software::scaling::Flags::BICUBIC,
                )
                .map_err(err)?,
            );
        }
        self.to_rgba
            .as_mut()
            .unwrap()
            .run(frame, &mut self.rgba)
            .map_err(err)?;
        let stride = self.rgba.stride(0);
        let data = self.rgba.data(0);
        let mut tight = Vec::with_capacity(sw as usize * sh as usize * 4);
        for row in 0..sh as usize {
            tight.extend_from_slice(&data[row * stride..row * stride + sw as usize * 4]);
        }
        let (turned, tw, _) = if plan.turn != 0 || plan.flip_h || plan.flip_v {
            orient(
                &tight,
                sw as usize,
                sh as usize,
                plan.turn,
                plan.flip_h,
                plan.flip_v,
            )
        } else {
            (tight, sw as usize, sh as usize)
        };
        let (cx, cy, cw, ch) = plan.crop_pixels(sw, sh);
        let (cx, cy, cw, ch) = (cx as usize, cy as usize, cw as usize, ch as usize);
        if self.staged.is_none() {
            self.staged = Some(ff::frame::Video::new(
                ff::format::Pixel::RGBA,
                cw as u32,
                ch as u32,
            ));
        }
        let staged = self.staged.as_mut().unwrap();
        let dst_stride = staged.stride(0);
        let dst = staged.data_mut(0);
        let fade = gain < 0.999;
        let mul = (gain * 256.0) as u32;
        for row in 0..ch {
            let from = ((cy + row) * tw + cx) * 4;
            let line = &turned[from..from + cw * 4];
            let to = &mut dst[row * dst_stride..row * dst_stride + cw * 4];
            if fade {
                for (d, s) in to
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(line.as_chunks::<4>().0.iter())
                {
                    d[0] = ((s[0] as u32 * mul) >> 8) as u8;
                    d[1] = ((s[1] as u32 * mul) >> 8) as u8;
                    d[2] = ((s[2] as u32 * mul) >> 8) as u8;
                    d[3] = 255;
                }
            } else {
                to.copy_from_slice(line);
            }
        }
        if let Some(ov) = plan.overlay.as_deref() {
            blend(dst, dst_stride, cw, ch, ov);
        }
        if self.to_nv12.is_none() {
            self.to_nv12 = Some(
                ff::software::scaling::Context::get(
                    ff::format::Pixel::RGBA,
                    cw as u32,
                    ch as u32,
                    ff::format::Pixel::NV12,
                    self.out.0,
                    self.out.1,
                    ff::software::scaling::Flags::BICUBIC,
                )
                .map_err(err)?,
            );
        }
        self.to_nv12
            .as_mut()
            .unwrap()
            .run(self.staged.as_ref().unwrap(), &mut self.nv12)
            .map_err(err)?;
        Ok(pack_nv12(&self.nv12, self.out))
    }
}

fn blend(dst: &mut [u8], stride: usize, w: usize, h: usize, ov: &Overlay) {
    let (ow, oh) = (ov.width as usize, ov.height as usize);
    if w == 0 || h == 0 || ow == 0 || oh == 0 || ov.rgba.len() < ow * oh * 4 {
        return;
    }
    let cols: Vec<usize> = (0..w).map(|x| x * ow / w).collect();
    for y in 0..h {
        let src = &ov.rgba[(y * oh / h) * ow * 4..][..ow * 4];
        let row = &mut dst[y * stride..y * stride + w * 4];
        for (d, sx) in row.as_chunks_mut::<4>().0.iter_mut().zip(&cols) {
            let s = &src[sx * 4..sx * 4 + 4];
            let a = s[3] as u32;
            if a == 0 {
                continue;
            }
            for c in 0..3 {
                d[c] = ((s[c] as u32 * a + d[c] as u32 * (255 - a) + 127) / 255) as u8;
            }
        }
    }
}

fn pack_nv12(frame: &ff::frame::Video, (w, h): (u32, u32)) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut nv12 = Vec::with_capacity(w * h * 3 / 2);
    let (s0, s1) = (frame.stride(0), frame.stride(1));
    let (d0, d1) = (frame.data(0), frame.data(1));
    for row in 0..h {
        nv12.extend_from_slice(&d0[row * s0..row * s0 + w]);
    }
    for row in 0..h / 2 {
        nv12.extend_from_slice(&d1[row * s1..row * s1 + w]);
    }
    nv12
}

fn decode_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(1, 8))
        .unwrap_or(2)
}

fn video_kbps(src: &Path, secs: f64, scale: f64) -> f64 {
    let bytes = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0) as f64;
    let total = bytes * 8.0 / secs.max(0.5) / 1000.0;
    ((total - AUDIO_KBPS as f64).max(MIN_KBPS) * scale.clamp(0.4, 1.0)).clamp(MIN_KBPS, MAX_KBPS)
}

pub fn render(src: &Path, dst: &Path, plan: &Plan, progress: &Progress) -> Result<Rendered> {
    render_painted(src, dst, plan, progress, None)
}

pub fn render_painted(
    src: &Path,
    dst: &Path,
    plan: &Plan,
    progress: &Progress,
    mut paint: Option<&mut Painter>,
) -> Result<Rendered> {
    crate::init()?;
    let info = crate::media_info(src)?;
    let mut plan = plan.clone();
    plan.clamp_to(info.duration);
    let fps = source_fps(src)?;
    let out = plan.output_size(info.width, info.height);
    let scale = (out.0 as f64 * out.1 as f64) / (info.width as f64 * info.height as f64).max(1.0);
    let kbps = video_kbps(src, info.duration, scale);
    let total_out = plan.output_secs().max(0.1);

    let audio = if info.audio_streams > 0 && plan.volume > 0.001 {
        let mix = decode_audio_mix(src, info.audio_streams);
        let mut edited = edit_audio(&mix, &plan);
        edited.truncate(((total_out * AUDIO_RATE as f64) as usize * 2).min(edited.len()) & !1);
        edited
    } else {
        Vec::new()
    };

    let mut ictx = ff::format::input(&src).map_err(err)?;
    let (index, params, tb, offset) = {
        let s = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
        let r = s.time_base();
        let tb = r.numerator() as f64 / r.denominator().max(1) as f64;
        let off = if s.start_time() > 0 {
            s.start_time() as f64 * tb
        } else {
            0.0
        };
        (s.index(), s.parameters(), tb, off)
    };
    let mut ctx = ff::codec::context::Context::from_parameters(params).map_err(err)?;
    ctx.set_threading(ff::threading::Config {
        kind: ff::threading::Type::Frame,
        count: decode_threads(),
    });
    let mut decoder = ctx.decoder().video().map_err(err)?;
    let (first_start, last_end) = plan.bounds();
    if first_start > 0.05 {
        let t = ((first_start + offset) * ff::ffi::AV_TIME_BASE as f64) as i64;
        let _ = ictx.seek(t, ..t);
    }

    let enc = Encoder::new(out.0, out.1, fps, kbps as u32, fps * 2, false)?;
    let vparams = enc.codec_params()?;
    let aac = if audio.is_empty() {
        None
    } else {
        Some(AacEncoder::new(AUDIO_RATE, 2, AUDIO_KBPS)?)
    };
    let mux = match &aac {
        Some(a) => Mp4Muxer::with_audio(dst, &vparams, &a.codec_params()?, a.rate())?,
        None => Mp4Muxer::from_params(dst, &vparams)?,
    };
    let mut sink = Sink {
        enc,
        mux,
        aac,
        audio,
        audio_pos: 0,
        last_ns: -1,
    };
    let mut conv = Converter::new(out);
    let pixels = plan.needs_pixels();
    let mut last_out = f64::NEG_INFINITY;
    let mut first_out: Option<f64> = None;
    let mut frame = ff::frame::Video::empty();
    let mut done = false;

    let mut handle = |frame: &ff::frame::Video, sink: &mut Sink| -> Result<bool> {
        let Some(p) = frame.pts() else {
            return Ok(true);
        };
        let t = p as f64 * tb - offset;
        if t >= last_end {
            return Ok(false);
        }
        let Some(out_t) = plan.output_time(t) else {
            return Ok(true);
        };
        let min_gap = if plan.speed_at(t) > 1.0 {
            0.9 / fps as f64
        } else {
            0.0
        };
        if out_t < last_out + min_gap {
            return Ok(true);
        }
        last_out = out_t;
        let mut nv12 = if pixels {
            conv.styled(frame, &plan, plan.fade_gain(out_t, total_out))?
        } else {
            conv.plain(frame)?
        };
        if let Some(paint) = paint.as_mut() {
            paint(
                t,
                &mut Canvas {
                    width: out.0 as usize,
                    height: out.1 as usize,
                    nv12: &mut nv12,
                },
            );
        }
        if first_out.is_none() {
            first_out = Some(out_t);
            sink.audio_pos = ((out_t * AUDIO_RATE as f64) as usize * 2).min(sink.audio.len()) & !1;
        }
        sink.push(&nv12, out_t)?;
        progress.set(out_t / total_out);
        Ok(true)
    };

    let mut failure: Option<Error> = None;
    'read: for (stream, packet) in ictx.packets() {
        if progress.cancelled() {
            failure = Some(Error::Ffmpeg(CANCELLED.into()));
            break;
        }
        if stream.index() != index || decoder.send_packet(&packet).is_err() {
            continue;
        }
        while decoder.receive_frame(&mut frame).is_ok() {
            match handle(&frame, &mut sink) {
                Ok(true) => {}
                Ok(false) => {
                    done = true;
                    break 'read;
                }
                Err(e) => {
                    failure = Some(e);
                    break 'read;
                }
            }
        }
    }
    if failure.is_none() && !done {
        let _ = decoder.send_eof();
        while decoder.receive_frame(&mut frame).is_ok() {
            match handle(&frame, &mut sink) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
    }
    if let Some(e) = failure {
        drop(sink);
        let _ = std::fs::remove_file(dst);
        return Err(e);
    }
    if first_out.is_none() {
        drop(sink);
        let _ = std::fs::remove_file(dst);
        return Err(Error::Ffmpeg("nothing to save in that selection".into()));
    }
    sink.finish()?;
    progress.set(1.0);
    let bytes = std::fs::metadata(dst).map(|m| m.len()).unwrap_or(0);
    Ok(Rendered {
        secs: total_out,
        bytes,
        width: out.0,
        height: out.1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{brightness, luma_at, make_clip};
    use sc_core::edit::{Crop, Segment};

    fn tone(secs: f32) -> Vec<f32> {
        let n = (48_000.0 * secs) as usize;
        (0..n).flat_map(|_| [0.5f32, 0.5f32]).collect()
    }

    fn grey(luma: f64) -> f64 {
        (luma - 16.0) * 1.164
    }

    fn workdir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-edit-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pieces(list: &[(f64, f64, f64)]) -> Plan {
        let mut plan = Plan::whole(6.0);
        plan.segments = list
            .iter()
            .map(|(a, b, s)| Segment {
                start: *a,
                end: *b,
                speed: *s,
            })
            .collect();
        plan
    }

    #[test]
    fn audio_follows_the_kept_pieces_and_their_speeds() {
        let mix = tone(10.0);
        let mut plan = pieces(&[(0.0, 2.0, 1.0), (6.0, 10.0, 1.0)]);
        let out = edit_audio(&mix, &plan);
        assert_eq!(out.len(), 6 * 48_000 * 2);
        plan.set_all_speeds(2.0);
        let fast = edit_audio(&mix, &plan);
        assert!((fast.len() as f64 - 3.0 * 48_000.0 * 2.0).abs() < 16.0);
        plan.set_speed(0, 0.5);
        let mixed = edit_audio(&mix, &plan);
        assert!((mixed.len() as f64 - (4.0 + 2.0) * 48_000.0 * 2.0).abs() < 16.0);
    }

    #[test]
    fn joins_are_faded_so_they_do_not_click() {
        let mix = tone(4.0);
        let plan = pieces(&[(0.0, 1.0, 1.0), (2.0, 3.0, 1.0)]);
        let out = edit_audio(&mix, &plan);
        let join = 48_000 * 2;
        assert!(out[join - 2].abs() < 0.01);
        assert!(out[join].abs() < 0.01);
        assert!(out[join / 2].abs() > 0.4);
    }

    #[test]
    fn volume_and_fades_shape_the_sound() {
        let mix = tone(4.0);
        let mut plan = Plan::whole(4.0);
        plan.volume = 0.5;
        plan.fade_in = 1.0;
        let out = edit_audio(&mix, &plan);
        assert!(out[0].abs() < 0.001);
        let mid = 2 * 48_000 * 2;
        assert!((out[mid] - 0.25).abs() < 0.001);
        plan.volume = 2.0;
        plan.fade_in = 0.0;
        let loud = edit_audio(&mix, &plan);
        assert!((loud[mid] - 1.0).abs() < 0.001);
    }

    #[test]
    fn fade_out_quietens_the_end_of_the_sound() {
        let mix = tone(4.0);
        let mut plan = Plan::whole(4.0);
        plan.fade_out = 2.0;
        let out = edit_audio(&mix, &plan);
        let at = |secs: f64| out[(secs * 48_000.0) as usize * 2].abs();
        assert!((at(1.0) - 0.5).abs() < 0.001);
        assert!((at(3.0) - 0.25).abs() < 0.01);
        assert!(at(3.99) < 0.01);
    }

    #[test]
    fn fades_darken_the_start_and_the_end_of_the_picture() {
        let dir = workdir("fade");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 6, 320, 180, true) {
            return;
        }
        let mut plan = Plan::whole(6.0);
        plan.fade_in = 2.0;
        plan.fade_out = 2.0;
        let dst = dir.join("faded.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let first = brightness(&dst, 0.0);
        let early = brightness(&dst, 1.0);
        let middle = brightness(&dst, 3.0);
        let late = brightness(&dst, 5.0);
        let last = brightness(&dst, 5.9);
        assert!(first < 30.0, "first {first}");
        assert!(early < middle - 20.0, "early {early} middle {middle}");
        assert!(late < middle - 20.0, "late {late} middle {middle}");
        assert!(last < early, "last {last} early {early}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cut_lands_exactly_where_it_was_placed() {
        let dir = workdir("exact");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 6, 320, 180, true) {
            return;
        }
        let plan = pieces(&[(1.2, 3.0, 1.0)]);
        let dst = dir.join("cut.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let info = crate::media_info(&dst).unwrap();
        assert!((info.duration - 1.8).abs() < 0.15, "{}", info.duration);
        let start = brightness(&dst, 0.0);
        let want = grey(luma_at(1.2, 6.0));
        assert!((start - want).abs() < 8.0, "start {start} want {want}");
        let end = brightness(&dst, 1.7);
        let want = grey(luma_at(2.9, 6.0));
        assert!((end - want).abs() < 8.0, "end {end} want {want}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_the_middle_joins_the_two_ends() {
        let dir = workdir("middle");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 6, 320, 180, true) {
            return;
        }
        let plan = pieces(&[(0.0, 2.0, 1.0), (4.0, 6.0, 1.0)]);
        let dst = dir.join("joined.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let info = crate::media_info(&dst).unwrap();
        assert!((info.duration - 4.0).abs() < 0.2, "{}", info.duration);
        assert_eq!(info.audio_streams, 1);
        let before = brightness(&dst, 1.8);
        let after = brightness(&dst, 2.3);
        assert!((before - grey(luma_at(1.8, 6.0))).abs() < 8.0, "{before}");
        assert!((after - grey(luma_at(4.3, 6.0))).abs() < 8.0, "{after}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fast_piece_plays_quicker_and_a_slow_piece_slower() {
        let dir = workdir("speed");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 6, 320, 180, true) {
            return;
        }
        let plan = pieces(&[(0.0, 2.0, 1.0), (2.0, 4.0, 2.0)]);
        let dst = dir.join("fast.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let info = crate::media_info(&dst).unwrap();
        assert!((info.duration - 3.0).abs() < 0.2, "{}", info.duration);
        assert_eq!(info.audio_streams, 1);
        let normal = brightness(&dst, 1.0);
        assert!((normal - grey(luma_at(1.0, 6.0))).abs() < 8.0, "{normal}");
        let fast = brightness(&dst, 2.5);
        assert!((fast - grey(luma_at(3.0, 6.0))).abs() < 8.0, "{fast}");

        let plan = pieces(&[(0.0, 2.0, 0.5)]);
        let slow = dir.join("slow.mp4");
        render(&src, &slow, &plan, &Progress::default()).unwrap();
        let info = crate::media_info(&slow).unwrap();
        assert!((info.duration - 4.0).abs() < 0.25, "{}", info.duration);
        let mid = brightness(&slow, 2.0);
        assert!((mid - grey(luma_at(1.0, 6.0))).abs() < 8.0, "{mid}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sound_of(path: &Path) -> (f64, f64) {
        let (_, start) = crate::first_pts(path).unwrap();
        let lead = start.unwrap_or(0.0);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let rx = crate::spawn_audio(path, 0.0, 0, stop);
        let (mut total, mut onset) = (0usize, None);
        for chunk in rx.iter() {
            if onset.is_none() {
                onset = chunk.iter().position(|v| v.abs() > 0.1).map(|i| total + i);
            }
            total += chunk.len();
        }
        let rate = AUDIO_RATE as f64 * 2.0;
        (
            lead + onset.map(|i| i as f64 / rate).unwrap_or(f64::NAN),
            lead + total as f64 / rate,
        )
    }

    #[test]
    fn moving_the_sound_later_delays_where_it_starts() {
        let dir = workdir("shift-later");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 4, 320, 180, true) {
            return;
        }
        let (onset, _) = sound_of(&src);
        assert!(onset < 0.05, "{onset}");
        let mut plan = Plan::whole(4.0);
        plan.audio_shift = 0.5;
        let dst = dir.join("later.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let (onset, end) = sound_of(&dst);
        assert!((onset - 0.5).abs() < 0.06, "{onset}");
        assert!((end - 4.0).abs() < 0.15, "{end}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn moving_the_sound_earlier_cuts_off_its_beginning() {
        let dir = workdir("shift-earlier");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 4, 320, 180, true) {
            return;
        }
        let mut plan = Plan::whole(4.0);
        plan.audio_shift = -0.5;
        let dst = dir.join("earlier.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let (onset, end) = sound_of(&dst);
        assert!(onset < 0.06, "{onset}");
        assert!((end - 3.5).abs() < 0.15, "{end}");
        let info = crate::media_info(&dst).unwrap();
        assert!((info.duration - 4.0).abs() < 0.25, "{}", info.duration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shift_larger_than_the_clip_leaves_silence_instead_of_failing() {
        let mix = tone(2.0);
        let mut plan = Plan::whole(2.0);
        plan.audio_shift = -3.0;
        let out = edit_audio(&mix, &plan);
        assert!(out.iter().all(|v| *v == 0.0));
        plan.audio_shift = 3.0;
        let out = edit_audio(&mix, &plan);
        assert_eq!(out.len(), 2 * 48_000 * 2);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_clip_without_sound_can_still_be_edited() {
        let dir = workdir("silent-source");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 4, 320, 180, false) {
            return;
        }
        let mut plan = pieces(&[(0.5, 3.5, 2.0)]);
        plan.fade_in = 0.5;
        let dst = dir.join("out.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let info = crate::media_info(&dst).unwrap();
        assert_eq!(info.audio_streams, 0);
        assert!((info.duration - 1.5).abs() < 0.2, "{}", info.duration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rendering_applies_the_cuts_crop_and_turn() {
        let dir = workdir("render");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 6, 320, 180, true) {
            return;
        }
        let mut plan = pieces(&[(0.5, 2.0, 1.5), (4.0, 5.5, 1.5)]);
        plan.turn = 90;
        plan.crop = Some(Crop {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 0.5,
        });
        let dst = dir.join("edited.mp4");
        let progress = Progress::default();
        let done = render(&src, &dst, &plan, &progress).unwrap();
        assert_eq!((done.width, done.height), (180, 160));
        let info = crate::media_info(&dst).unwrap();
        assert_eq!((info.width, info.height), (180, 160));
        assert_eq!(info.audio_streams, 1);
        assert!(
            (info.duration - 2.0).abs() < 0.25,
            "duration {}",
            info.duration
        );
        assert_eq!(progress.fraction(), 1.0);

        plan.volume = 0.0;
        let silent = dir.join("silent.mp4");
        render(&src, &silent, &plan, &progress).unwrap();
        assert_eq!(crate::media_info(&silent).unwrap().audio_streams, 0);

        let stop = Progress::default();
        stop.cancel();
        let never = dir.join("never.mp4");
        let err = render(&src, &never, &plan, &stop).unwrap_err();
        assert!(err.to_string().contains(CANCELLED));
        assert!(!never.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn block(w: u32, h: u32) -> Overlay {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        rgba[..(w * h) as usize].fill(255);
        Overlay {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn an_overlay_is_blended_and_scaled_to_the_picture() {
        let mut px = vec![100u8; 4 * 4 * 4];
        let mut ov = block(2, 2);
        ov.rgba[3] = 128;
        blend(&mut px, 16, 4, 4, &ov);
        assert_eq!(&px[..4], &[178, 178, 178, 100]);
        assert_eq!(&px[4..8], &[178, 178, 178, 100]);
        assert_eq!(px[8], 100);
        assert_eq!(px[16], 178);
        assert_eq!(px[32], 100);
        let mut same = vec![9u8; 16];
        blend(&mut same, 8, 2, 2, &block(0, 0));
        assert!(same.iter().all(|v| *v == 9));
    }

    #[test]
    fn an_overlay_brightens_only_where_it_is_drawn() {
        let dir = workdir("overlay");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 2, 320, 180, true) {
            return;
        }
        let mut plan = Plan::whole(2.0);
        let mut ov = block(320, 180);
        ov.rgba.fill(0);
        for p in ov.rgba[..320 * 45 * 4].as_chunks_mut::<4>().0 {
            *p = [255, 255, 255, 255];
        }
        plan.overlay = Some(std::sync::Arc::new(ov));
        assert!(plan.needs_pixels());
        let dst = dir.join("overlay.mp4");
        render(&src, &dst, &plan, &Progress::default()).unwrap();
        let mean = |path: &Path, rows: std::ops::Range<usize>| {
            let (w, _, rgb) = crate::frame_at(path, 1.0, 320).unwrap();
            let w = w as usize * 3;
            let part = &rgb[rows.start * w..rows.end * w];
            part.iter().map(|v| *v as f64).sum::<f64>() / part.len() as f64
        };
        let (top, low) = (mean(&dst, 5..35), mean(&dst, 100..170));
        let (was_top, was_low) = (mean(&src, 5..35), mean(&src, 100..170));
        assert!(
            top > 240.0 && top > was_top + 60.0,
            "top {top} was {was_top}"
        );
        assert!((low - was_low).abs() < 8.0, "low {low} was {was_low}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamps_land_in_the_right_colours_and_stay_inside() {
        let (w, h) = (8usize, 4usize);
        let mut nv12 = vec![16u8; w * h];
        nv12.extend(std::iter::repeat_n(128u8, w * h / 2));
        let red = Stamp::from_rgba(2, 2, &[255, 0, 0, 255].repeat(4));
        let mut c = Canvas {
            width: w,
            height: h,
            nv12: &mut nv12,
        };
        c.stamp(2, 0, &red);
        c.stamp(7, 3, &red);
        c.stamp(-9, -9, &red);
        let soft = Stamp::from_rgba(2, 2, &[255, 255, 255, 128].repeat(4));
        c.stamp(4, 2, &soft);
        assert_eq!(nv12[2], 63);
        assert_eq!(nv12[w + 3], 63);
        assert_eq!(nv12[0], 16);
        let (u, v) = (nv12[w * h + 2], nv12[w * h + 3]);
        assert!(u < 110 && v > 235, "u {u} v {v}");
        let half = nv12[2 * w + 4];
        assert!((120..=130).contains(&half), "{half}");
        assert_eq!(nv12[3 * w + 6], 63, "clipped at the corner");
    }

    #[test]
    fn painting_follows_the_clock_and_the_sound_is_copied_back() {
        let dir = workdir("painted");
        let src = dir.join("source.mp4");
        if !make_clip(&src, 4, 320, 180, true) {
            return;
        }
        let mut plan = Plan::whole(4.0);
        plan.volume = 0.0;
        let mut seen = Vec::new();
        let white = Stamp::from_rgba(320, 45, &[255u8; 320 * 45 * 4]);
        let mut paint = |t: f64, c: &mut Canvas| {
            seen.push(t);
            if t < 2.0 {
                return;
            }
            c.stamp(0, 0, &white);
        };
        let picture = dir.join("picture.mp4");
        render_painted(
            &src,
            &picture,
            &plan,
            &Progress::default(),
            Some(&mut paint),
        )
        .unwrap();
        assert!(seen.windows(2).all(|w| w[1] > w[0]), "{seen:?}");
        assert!(seen[0] < 0.05 && *seen.last().unwrap() > 3.8);
        assert_eq!(crate::media_info(&picture).unwrap().audio_streams, 0);
        let dst = dir.join("joined.mp4");
        crate::with_sound_of(&picture, &src, &dst).unwrap();
        let info = crate::media_info(&dst).unwrap();
        assert_eq!(info.audio_streams, 1);
        assert!((info.duration - 4.0).abs() < 0.15, "{}", info.duration);
        let (_, a) = crate::first_pts(&dst).unwrap();
        let (_, was) = crate::first_pts(&src).unwrap();
        assert!((a.unwrap_or(0.0) - was.unwrap_or(0.0)).abs() < 0.03);
        let top = |at: f64| {
            let (w, _, rgb) = crate::frame_at(&dst, at, 320).unwrap();
            let part = &rgb[5 * w as usize * 3..35 * w as usize * 3];
            part.iter().map(|v| *v as f64).sum::<f64>() / part.len() as f64
        };
        let (early, late) = (top(1.0), top(3.0));
        assert!(
            late > 240.0 && late > early + 60.0,
            "early {early} late {late}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bitrate_follows_the_source_and_stays_in_range() {
        let kbps = video_kbps(Path::new("missing.mp4"), 10.0, 1.0);
        assert_eq!(kbps, MIN_KBPS);
    }
}
