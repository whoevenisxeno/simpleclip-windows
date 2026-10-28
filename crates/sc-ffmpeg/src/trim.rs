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
