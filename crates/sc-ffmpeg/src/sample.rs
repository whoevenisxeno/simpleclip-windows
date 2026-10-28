use crate::{AacEncoder, Encoder, Mp4Muxer, AUDIO_RATE};
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::time::Timestamp;
use std::path::Path;

pub const FPS: u32 = 30;
pub const FIRST_LUMA: u32 = 40;
pub const LUMA_RANGE: u32 = 160;

pub fn luma_at(secs: f64, total: f64) -> f64 {
    FIRST_LUMA as f64 + LUMA_RANGE as f64 * secs / total
}

pub fn make_clip(path: &Path, secs: u32, w: u32, h: u32, audio: bool) -> bool {
    let Ok(mut enc) = Encoder::new(w, h, FPS, 3_000, FPS * 2, false) else {
        return false;
    };
    let Ok(vparams) = enc.codec_params() else {
        return false;
    };
    let mut aac = if audio {
        match AacEncoder::new(AUDIO_RATE, 2, 96) {
            Ok(a) => Some(a),
            Err(_) => return false,
        }
    } else {
        None
    };
    let muxed = match &aac {
        Some(a) => match a.codec_params() {
            Ok(p) => Mp4Muxer::with_audio(path, &vparams, &p, a.rate()),
            Err(_) => return false,
        },
        None => Mp4Muxer::from_params(path, &vparams),
    };
    let Ok(mut mux) = muxed else {
        return false;
    };
    let frames = secs * FPS;
    for n in 0..frames {
        let mut nv12 = vec![0u8; (w * h * 3 / 2) as usize];
        let luma = (FIRST_LUMA + n * LUMA_RANGE / frames) as u8;
        nv12[..(w * h) as usize].fill(luma);
        nv12[(w * h) as usize..].fill(128);
        let ns = n as i64 * 1_000_000_000 / FPS as i64;
        let Ok(packets) = enc.push_nv12(&nv12, ns) else {
            return false;
        };
        for p in packets {
            if mux.write(&p).is_err() {
                return false;
            }
        }
    }
    let Ok(packets) = enc.flush() else {
        return false;
    };
    for p in packets {
        if mux.write(&p).is_err() {
            return false;
        }
    }
    if let Some(aac) = aac.as_mut() {
        let total = (AUDIO_RATE * secs) as usize;
        let samples: Vec<f32> = (0..total)
            .flat_map(|i| {
                let v =
                    (2.0 * std::f32::consts::PI * 440.0 * i as f32 / AUDIO_RATE as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        for (i, chunk) in samples.chunks(2048).enumerate() {
            let t = (i * 2048 / 2) as f64 / AUDIO_RATE as f64;
            let buf = AudioBuffer {
                role: AudioRole::Desktop,
                sample_rate: AUDIO_RATE,
                channels: 2,
                timestamp: Timestamp::from_nanos((t * 1e9) as i64),
                samples: chunk.to_vec(),
            };
            let Ok(packets) = aac.push(&buf) else {
                return false;
            };
            for p in packets {
                if mux.write_audio(&p).is_err() {
                    return false;
                }
            }
        }
        let Ok(packets) = aac.flush() else {
            return false;
        };
        for p in packets {
            if mux.write_audio(&p).is_err() {
                return false;
            }
        }
    }
    mux.finish().is_ok()
}

pub fn brightness(path: &Path, at: f64) -> f64 {
    let (_, _, rgb) = crate::frame_at(path, at, 160).unwrap();
    rgb.iter().map(|v| *v as f64).sum::<f64>() / rgb.len() as f64
}
