use crate::{Error, Result};
use ffmpeg_next as ff;
use std::path::Path;

fn err(e: ff::Error) -> Error {
    Error::Ffmpeg(e.to_string())
}

pub fn thumbnail(path: &Path, max_width: u32) -> Result<(u32, u32, Vec<u8>)> {
    match decode(path, max_width, Target::Quarter) {
        Ok(t) => Ok(t),
        Err(_) => decode(path, max_width, Target::First),
    }
}

pub fn frame_at(path: &Path, secs: f64, max_width: u32) -> Result<(u32, u32, Vec<u8>)> {
    match decode(path, max_width, Target::At(secs.max(0.0))) {
        Ok(t) => Ok(t),
        Err(_) => decode(path, max_width, Target::First),
    }
}

#[derive(Clone, Copy)]
enum Target {
    First,
    Quarter,
    At(f64),
}

fn decode(path: &Path, max_width: u32, target: Target) -> Result<(u32, u32, Vec<u8>)> {
    crate::init()?;
    let mut ictx = ff::format::input(&path).map_err(err)?;
    let (index, params, tb) = {
        let stream = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
        let r = stream.time_base();
        (
            stream.index(),
            stream.parameters(),
            r.numerator() as f64 / r.denominator().max(1) as f64,
        )
    };
    let mut decoder = ff::codec::context::Context::from_parameters(params)
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;

    let (seek_us, want_secs) = match target {
        Target::First => (None, None),
        Target::Quarter => (Some(ictx.duration() / 4), None),
        Target::At(s) => (Some((s * ff::ffi::AV_TIME_BASE as f64) as i64), Some(s)),
    };
    if let Some(t) = seek_us.filter(|t| *t > 0) {
        let _ = ictx.seek(t, ..t);
    }

    let mut frame = ff::frame::Video::empty();
    let mut got = false;
    let mut decoded = 0u32;
    let wanted = |f: &ff::frame::Video| match want_secs {
        None => true,
        Some(s) => f.pts().is_none_or(|p| p as f64 * tb >= s - 0.001),
    };
    'packets: for (stream, packet) in ictx.packets() {
        if stream.index() != index || decoder.send_packet(&packet).is_err() {
            continue;
        }
        while decoder.receive_frame(&mut frame).is_ok() {
            decoded += 1;
            if wanted(&frame) || decoded > 900 {
                got = true;
                break 'packets;
            }
        }
    }
    if !got {
        let _ = decoder.send_eof();
        let mut last = ff::frame::Video::empty();
        while decoder.receive_frame(&mut last).is_ok() {
            frame = last.clone();
            got = true;
            if wanted(&frame) {
                break;
            }
        }
    }
    if !got {
        return Err(Error::Ffmpeg("no frame decoded".into()));
    }

    let (sw, sh) = (frame.width(), frame.height());
    let tw = max_width.min(sw).max(2) & !1;
    let th = ((sh as u64 * tw as u64 / sw.max(1) as u64) as u32).max(2) & !1;
    let mut scaler = ff::software::scaling::Context::get(
        frame.format(),
        sw,
        sh,
        ff::format::Pixel::RGB24,
        tw,
        th,
        ff::software::scaling::Flags::BILINEAR,
    )
    .map_err(err)?;
    let mut rgb = ff::frame::Video::empty();
    scaler.run(&frame, &mut rgb).map_err(err)?;

    let stride = rgb.stride(0);
    let data = rgb.data(0);
    let mut out = Vec::with_capacity((tw * th * 3) as usize);
    for row in 0..th as usize {
        out.extend_from_slice(&data[row * stride..row * stride + tw as usize * 3]);
    }
    Ok((tw, th, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_an_error() {
        assert!(thumbnail(Path::new("no-such-file.mp4"), 320).is_err());
        assert!(frame_at(Path::new("no-such-file.mp4"), 3.0, 320).is_err());
    }
}
