//! Preview frames for the gallery: decode one frame from a clip and scale it
//! down to RGB.

use crate::{Error, Result};
use ffmpeg_next as ff;
use std::path::Path;

fn err(e: ff::Error) -> Error {
    Error::Ffmpeg(e.to_string())
}

/// Decode a representative frame (about a quarter of the way in, so it is
/// neither the loading frame nor the tail) and scale it to at most `max_width`
/// pixels wide. Returns `(width, height, rgb24 bytes)`, tightly packed.
pub fn thumbnail(path: &Path, max_width: u32) -> Result<(u32, u32, Vec<u8>)> {
    crate::init()?;
    let mut ictx = ff::format::input(&path).map_err(err)?;
    let (index, params) = {
        let stream = ictx
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| Error::Ffmpeg("no video stream".into()))?;
        (stream.index(), stream.parameters())
    };
    let mut decoder = ff::codec::context::Context::from_parameters(params)
        .map_err(err)?
        .decoder()
        .video()
        .map_err(err)?;

    // Seek lands on the keyframe at or before the target.
    let target = ictx.duration() / 4;
    if target > 0 {
        let _ = ictx.seek(target, ..target);
    }

    let mut frame = ff::frame::Video::empty();
    let mut got = false;
    for (stream, packet) in ictx.packets() {
        if stream.index() != index {
            continue;
        }
        if decoder.send_packet(&packet).is_err() {
            continue;
        }
        if decoder.receive_frame(&mut frame).is_ok() {
            got = true;
            break;
        }
    }
    if !got {
        // Flush any frames the decoder is still holding (very short clips).
        let _ = decoder.send_eof();
        got = decoder.receive_frame(&mut frame).is_ok();
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
    }
}
