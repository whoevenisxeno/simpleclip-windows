//! Proves the VA-API H.264 encode + MP4 mux path with synthetic frames, no
//! display or capture backend needed. Run on a machine with a VA-API GPU:
//!   cargo run -p sc-ffmpeg --example encode_synthetic -- /tmp/out.mp4
//! Then verify with: ffprobe /tmp/out.mp4

#[cfg(target_os = "linux")]
fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/sc-synthetic.mp4".into());
    let (w, h, fps, secs) = (1280u32, 720u32, 60u32, 3u32);

    let mut rec = sc_ffmpeg::Recorder::new(std::path::Path::new(&path), w, h, fps, 8_000, 120)
        .expect("open recorder");

    let frame_count = fps * secs;
    for i in 0..frame_count {
        let nv12 = moving_bars(w as usize, h as usize, i);
        rec.push_nv12(&nv12).expect("push frame");
    }
    rec.finish().expect("finish");
    println!("wrote {frame_count} frames to {path}");
}

/// A scrolling luma gradient + shifting chroma so the encoder has real motion to
/// compress and A/V/quality can be eyeballed.
#[cfg(target_os = "linux")]
fn moving_bars(w: usize, h: usize, frame: u32) -> Vec<u8> {
    let mut buf = vec![0u8; w * h + w * h / 2];
    for y in 0..h {
        for x in 0..w {
            buf[y * w + x] = (((x + frame as usize * 4) & 0xff) ^ (y & 0xff)) as u8;
        }
    }
    let uv = &mut buf[w * h..];
    for (i, chunk) in uv.chunks_mut(2).enumerate() {
        chunk[0] = (i as u32 + frame * 2) as u8;
        chunk[1] = (i as u32 * 2 + frame) as u8;
    }
    buf
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("encode_synthetic is Linux-only (VA-API)");
}
