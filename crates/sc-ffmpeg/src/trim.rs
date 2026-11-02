use crate::{Error, Result};
use ffmpeg_next::ffi;
use std::ffi::CString;
use std::path::Path;
use std::ptr;

const US: ffi::AVRational = ffi::AVRational {
    num: 1,
    den: 1_000_000,
};

fn cpath(p: &Path) -> Result<CString> {
    CString::new(p.to_string_lossy().as_bytes())
        .map_err(|_| Error::Ffmpeg("path has interior NUL".into()))
}

struct Input(*mut ffi::AVFormatContext);

impl Input {
    fn open(path: &Path) -> Result<Self> {
        crate::init()?;
        let p = cpath(path)?;
        let mut ic: *mut ffi::AVFormatContext = ptr::null_mut();
        unsafe {
            let rc = ffi::avformat_open_input(&mut ic, p.as_ptr(), ptr::null(), ptr::null_mut());
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "avformat_open_input",
                });
            }
            let this = Self(ic);
            let rc = ffi::avformat_find_stream_info(ic, ptr::null_mut());
            if rc < 0 {
                return Err(Error::Av {
                    code: rc,
                    ctx: "avformat_find_stream_info",
                });
            }
            Ok(this)
        }
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        unsafe { ffi::avformat_close_input(&mut self.0) };
    }
}

pub fn probe_duration(path: &Path) -> Result<f64> {
    let input = Input::open(path)?;
    let d = unsafe { (*input.0).duration };
    if d <= 0 {
        return Err(Error::Ffmpeg("file has no duration".into()));
    }
    Ok(d as f64 / ffi::AV_TIME_BASE as f64)
}

pub fn trim_copy(src: &Path, dst: &Path, start_secs: f64, end_secs: f64) -> Result<f64> {
    if end_secs <= start_secs {
        return Err(Error::Ffmpeg("trim end must be after start".into()));
    }
    let input = Input::open(src)?;
    let dst_c = cpath(dst)?;
    unsafe {
        let ic = input.0;
        let nb = (*ic).nb_streams as usize;

        let mut oc: *mut ffi::AVFormatContext = ptr::null_mut();
        let fmt = CString::new("mp4").unwrap();
        let rc =
            ffi::avformat_alloc_output_context2(&mut oc, ptr::null(), fmt.as_ptr(), dst_c.as_ptr());
        if rc < 0 || oc.is_null() {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_alloc_output_context2",
            });
        }
        let out = Output(oc);

        let mut map = vec![-1i32; nb];
        let mut video_in: Option<usize> = None;
        #[allow(clippy::needless_range_loop)]
        for i in 0..nb {
            let st = *(*ic).streams.add(i);
            let ty = (*(*st).codecpar).codec_type;
            if ty != ffi::AVMediaType::AVMEDIA_TYPE_VIDEO
                && ty != ffi::AVMediaType::AVMEDIA_TYPE_AUDIO
            {
                continue;
            }
            let ost = ffi::avformat_new_stream(oc, ptr::null());
            if ost.is_null() {
                return Err(Error::Av {
                    code: -1,
                    ctx: "avformat_new_stream",
                });
            }
            if ffi::avcodec_parameters_copy((*ost).codecpar, (*st).codecpar) < 0 {
                return Err(Error::Av {
                    code: -1,
                    ctx: "avcodec_parameters_copy",
                });
            }
            (*(*ost).codecpar).codec_tag = 0;
            (*ost).time_base = (*st).time_base;
            map[i] = (*ost).index;
            if ty == ffi::AVMediaType::AVMEDIA_TYPE_VIDEO && video_in.is_none() {
                video_in = Some(i);
            }
        }
        let video_in = video_in.ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;

        if ffi::avio_open(&mut (*oc).pb, dst_c.as_ptr(), ffi::AVIO_FLAG_WRITE) < 0 {
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
        let rc = ffi::avformat_write_header(oc, &mut opts);
        ffi::av_dict_free(&mut opts);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_write_header",
            });
        }

        let start_us = (start_secs.max(0.0) * 1_000_000.0) as i64;
        let end_us = (end_secs * 1_000_000.0) as i64;
        let rc = ffi::av_seek_frame(
            ic,
            video_in as i32,
            ffi::av_rescale_q(start_us, US, (**(*ic).streams.add(video_in)).time_base),
            ffi::AVSEEK_FLAG_BACKWARD,
        );
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "av_seek_frame",
            });
        }

        let mut pkt = ffi::av_packet_alloc();
        let mut base_us: Option<i64> = None;
        let mut last_us = 0i64;
        let result = loop {
            if ffi::av_read_frame(ic, pkt) < 0 {
                break Ok(());
            }
            let idx = (*pkt).stream_index as usize;
            let oidx = map.get(idx).copied().unwrap_or(-1);
            if oidx < 0 {
                ffi::av_packet_unref(pkt);
                continue;
            }
            let in_tb = (**(*ic).streams.add(idx)).time_base;
            let ts = if (*pkt).pts != ffi::AV_NOPTS_VALUE {
                (*pkt).pts
            } else {
                (*pkt).dts
            };
            let ts_us = ffi::av_rescale_q(ts, in_tb, US);

            if base_us.is_none() {
                if idx != video_in {
                    ffi::av_packet_unref(pkt);
                    continue;
                }
                base_us = Some(ts_us);
            }
            let base = base_us.unwrap();
            if ts_us < base {
                ffi::av_packet_unref(pkt);
                continue;
            }
            if ts_us > end_us {
                ffi::av_packet_unref(pkt);
                if idx == video_in {
                    break Ok(());
                }
                continue;
            }
            let out_tb = (**(*oc).streams.add(oidx as usize)).time_base;
            let off = ffi::av_rescale_q(base, US, in_tb);
            if (*pkt).pts != ffi::AV_NOPTS_VALUE {
                (*pkt).pts = ffi::av_rescale_q((*pkt).pts - off, in_tb, out_tb);
            }
            if (*pkt).dts != ffi::AV_NOPTS_VALUE {
                (*pkt).dts = ffi::av_rescale_q((*pkt).dts - off, in_tb, out_tb);
            }
            (*pkt).duration = ffi::av_rescale_q((*pkt).duration, in_tb, out_tb);
            (*pkt).stream_index = oidx;
            (*pkt).pos = -1;
            last_us = last_us.max(ts_us - base);
            let rc = ffi::av_interleaved_write_frame(oc, pkt);
            ffi::av_packet_unref(pkt);
            if rc < 0 {
                break Err(Error::Av {
                    code: rc,
                    ctx: "av_interleaved_write_frame",
                });
            }
        };
        ffi::av_packet_free(&mut pkt);
        result?;
        if ffi::av_write_trailer(oc) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "av_write_trailer",
            });
        }
        drop(out);
        Ok(last_us as f64 / 1_000_000.0)
    }
}

unsafe fn start_secs(ic: *mut ffi::AVFormatContext, index: usize) -> f64 {
    let st = *(*ic).streams.add(index);
    let tb = (*st).time_base;
    if (*st).start_time == ffi::AV_NOPTS_VALUE || (*st).start_time <= 0 {
        return 0.0;
    }
    (*st).start_time as f64 * tb.num as f64 / tb.den.max(1) as f64
}

unsafe fn streams_of(ic: *mut ffi::AVFormatContext, kind: ffi::AVMediaType) -> Vec<usize> {
    (0..(*ic).nb_streams as usize)
        .filter(|&i| (*(**(*ic).streams.add(i)).codecpar).codec_type == kind)
        .collect()
}

struct Feed {
    ic: *mut ffi::AVFormatContext,
    map: Vec<i32>,
    shift: f64,
    pkt: *mut ffi::AVPacket,
    ready: Option<f64>,
    done: bool,
}

impl Feed {
    unsafe fn secs(&self, ts: i64, idx: usize) -> f64 {
        let tb = (**(*self.ic).streams.add(idx)).time_base;
        ts as f64 * tb.num as f64 / tb.den.max(1) as f64 - self.shift
    }

    unsafe fn next(&mut self) {
        self.ready = None;
        while !self.done {
            if ffi::av_read_frame(self.ic, self.pkt) < 0 {
                self.done = true;
                return;
            }
            let idx = (*self.pkt).stream_index as usize;
            let (pts, dts) = ((*self.pkt).pts, (*self.pkt).dts);
            let ts = if dts != ffi::AV_NOPTS_VALUE { dts } else { pts };
            if self.map.get(idx).copied().unwrap_or(-1) < 0 || ts == ffi::AV_NOPTS_VALUE {
                ffi::av_packet_unref(self.pkt);
                continue;
            }
            let shown = if pts != ffi::AV_NOPTS_VALUE { pts } else { ts };
            if self.secs(shown, idx) < -0.0005 {
                ffi::av_packet_unref(self.pkt);
                continue;
            }
            self.ready = Some(self.secs(ts, idx));
            return;
        }
    }

    unsafe fn write(&mut self, oc: *mut ffi::AVFormatContext) -> Result<()> {
        let idx = (*self.pkt).stream_index as usize;
        let oidx = self.map[idx];
        let in_tb = (**(*self.ic).streams.add(idx)).time_base;
        let out_tb = (**(*oc).streams.add(oidx as usize)).time_base;
        let off = ffi::av_rescale_q((self.shift * 1_000_000.0).round() as i64, US, in_tb);
        if (*self.pkt).pts != ffi::AV_NOPTS_VALUE {
            (*self.pkt).pts = ffi::av_rescale_q((*self.pkt).pts - off, in_tb, out_tb);
        }
        if (*self.pkt).dts != ffi::AV_NOPTS_VALUE {
            (*self.pkt).dts = ffi::av_rescale_q((*self.pkt).dts - off, in_tb, out_tb);
        }
        (*self.pkt).duration = ffi::av_rescale_q((*self.pkt).duration, in_tb, out_tb);
        (*self.pkt).stream_index = oidx;
        (*self.pkt).pos = -1;
        let rc = ffi::av_interleaved_write_frame(oc, self.pkt);
        ffi::av_packet_unref(self.pkt);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "av_interleaved_write_frame",
            });
        }
        Ok(())
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        unsafe { ffi::av_packet_free(&mut self.pkt) };
    }
}

unsafe fn copy_stream(oc: *mut ffi::AVFormatContext, st: *mut ffi::AVStream) -> Result<i32> {
    let ost = ffi::avformat_new_stream(oc, ptr::null());
    if ost.is_null() {
        return Err(Error::Av {
            code: -1,
            ctx: "avformat_new_stream",
        });
    }
    if ffi::avcodec_parameters_copy((*ost).codecpar, (*st).codecpar) < 0 {
        return Err(Error::Av {
            code: -1,
            ctx: "avcodec_parameters_copy",
        });
    }
    (*(*ost).codecpar).codec_tag = 0;
    (*ost).time_base = (*st).time_base;
    (*ost).disposition = (*st).disposition;
    if !(*st).metadata.is_null() {
        ffi::av_dict_copy(&mut (*ost).metadata, (*st).metadata, 0);
    }
    Ok((*ost).index)
}

pub fn with_sound_of(video: &Path, sound: &Path, dst: &Path) -> Result<()> {
    let pic = Input::open(video)?;
    let snd = Input::open(sound)?;
    let dst_c = cpath(dst)?;
    unsafe {
        let mut oc: *mut ffi::AVFormatContext = ptr::null_mut();
        let fmt = CString::new("mp4").unwrap();
        let rc =
            ffi::avformat_alloc_output_context2(&mut oc, ptr::null(), fmt.as_ptr(), dst_c.as_ptr());
        if rc < 0 || oc.is_null() {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_alloc_output_context2",
            });
        }
        let out = Output(oc);

        let v = *streams_of(pic.0, ffi::AVMediaType::AVMEDIA_TYPE_VIDEO)
            .first()
            .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
        let mut vmap = vec![-1i32; (*pic.0).nb_streams as usize];
        vmap[v] = copy_stream(oc, *(*pic.0).streams.add(v))?;
        let mut amap = vec![-1i32; (*snd.0).nb_streams as usize];
        for a in streams_of(snd.0, ffi::AVMediaType::AVMEDIA_TYPE_AUDIO) {
            amap[a] = copy_stream(oc, *(*snd.0).streams.add(a))?;
        }
        let sound_shift = streams_of(snd.0, ffi::AVMediaType::AVMEDIA_TYPE_VIDEO)
            .first()
            .map(|&i| start_secs(snd.0, i))
            .unwrap_or(0.0);

        if ffi::avio_open(&mut (*oc).pb, dst_c.as_ptr(), ffi::AVIO_FLAG_WRITE) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "avio_open",
            });
        }
        let mut opts: *mut ffi::AVDictionary = ptr::null_mut();
        let (k, val) = (
            CString::new("movflags").unwrap(),
            CString::new("+faststart").unwrap(),
        );
        ffi::av_dict_set(&mut opts, k.as_ptr(), val.as_ptr(), 0);
        let rc = ffi::avformat_write_header(oc, &mut opts);
        ffi::av_dict_free(&mut opts);
        if rc < 0 {
            return Err(Error::Av {
                code: rc,
                ctx: "avformat_write_header",
            });
        }

        let mut feeds = [
            Feed {
                ic: pic.0,
                map: vmap,
                shift: start_secs(pic.0, v),
                pkt: ffi::av_packet_alloc(),
                ready: None,
                done: false,
            },
            Feed {
                ic: snd.0,
                map: amap,
                shift: sound_shift,
                pkt: ffi::av_packet_alloc(),
                ready: None,
                done: false,
            },
        ];
        for f in feeds.iter_mut() {
            f.next();
        }
        loop {
            let pick = match (feeds[0].ready, feeds[1].ready) {
                (Some(a), Some(b)) => usize::from(b < a),
                (Some(_), None) => 0,
                (None, Some(_)) => 1,
                (None, None) => break,
            };
            feeds[pick].write(oc)?;
            feeds[pick].next();
        }
        drop(feeds);
        if ffi::av_write_trailer(oc) < 0 {
            return Err(Error::Av {
                code: -1,
                ctx: "av_write_trailer",
            });
        }
        drop(out);
    }
    Ok(())
}

struct Output(*mut ffi::AVFormatContext);

impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                if !(*self.0).pb.is_null() {
                    ffi::avio_closep(&mut (*self.0).pb);
                }
                ffi::avformat_free_context(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_rejects_inverted_range() {
        let p = Path::new("nonexistent.mp4");
        assert!(trim_copy(p, p, 5.0, 1.0).is_err());
    }

    #[test]
    fn probe_reports_missing_file() {
        assert!(probe_duration(Path::new("definitely-not-here.mp4")).is_err());
    }
}
