//! Full Phase 1 video pipeline on real screen content: portal+PipeWire capture
//! -> BGRA->NV12 -> VA-API H.264 -> MP4. Pops the screen-share dialog.
//!   cargo run -p sc-capture --example record_screen -- /tmp/screen.mp4 5
//! (second arg = seconds to record). Verify with: ffprobe /tmp/screen.mp4

#[cfg(target_os = "linux")]
fn main() {
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "/tmp/sc-screen.mp4".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);

    let (tx, rx) = crossbeam_channel::bounded(120);
    let cap = sc_capture::start(tx).expect("start capture");
    let (w, h) = (cap.width, cap.height);
    println!("capturing {w}x{h} for {secs}s -> {path}");

    let mut conv = sc_ffmpeg::Bgra2Nv12::new(w, h).expect("converter");
    let mut rec = sc_ffmpeg::Recorder::new(std::path::Path::new(&path), w, h, 60, 20_000, 120)
        .expect("recorder");

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut n = 0u32;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(frame) => {
                let nv12 = conv
                    .convert(&frame.data, frame.stride as usize)
                    .expect("convert");
                rec.push_nv12(&nv12).expect("encode");
                n += 1;
            }
            Err(_) => continue,
        }
    }
    drop(cap);
    rec.finish().expect("finish");
    println!("recorded {n} frames to {path}");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("record_screen is Linux-only");
}
