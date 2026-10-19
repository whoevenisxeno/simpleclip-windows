//! Decode a clip through the player path and report what came out.
//! `cargo run -p sc-ffmpeg --example play_probe -- <clip> [start_secs]`
use std::sync::{atomic::AtomicBool, Arc};

fn main() {
    let mut a = std::env::args().skip(1);
    let path = std::path::PathBuf::from(a.next().expect("clip path"));
    let start: f64 = a.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let info = sc_ffmpeg::media_info(&path).unwrap();
    println!(
        "{}x{} {:.2}s audio={}",
        info.width, info.height, info.duration, info.has_audio
    );
    let stop = Arc::new(AtomicBool::new(false));
    let v = sc_ffmpeg::spawn_video(&path, start, 960, stop.clone());
    let au = sc_ffmpeg::spawn_audio(&path, start, stop);
    let (mut n, mut first, mut last) = (0, f64::NAN, 0.0);
    for f in v.iter() {
        if n == 0 {
            first = f.pts;
            println!("frame {}x{} bytes={}", f.width, f.height, f.rgba.len());
        }
        last = f.pts;
        n += 1;
    }
    let samples: usize = au.iter().map(|c| c.len()).sum();
    println!(
        "video: {n} frames, pts {first:.3}..{last:.3}; audio: {:.2}s",
        samples as f64 / 2.0 / sc_ffmpeg::AUDIO_RATE as f64
    );
}
