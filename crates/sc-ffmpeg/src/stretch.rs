const WINDOW: usize = 1920;
const HOP: usize = WINDOW / 2;
const SEARCH: isize = 480;
const DECIMATE: usize = 4;

fn hann() -> Vec<f32> {
    (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / WINDOW as f32).cos())
        .collect()
}

fn sample(input: &[f32], frame: isize, ch: usize) -> f32 {
    if frame < 0 {
        return 0.0;
    }
    input.get(frame as usize * 2 + ch).copied().unwrap_or(0.0)
}

fn resample(input: &[f32], speed: f64) -> Vec<f32> {
    let frames = input.len() / 2;
    let out_frames = (frames as f64 / speed).floor() as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = i as f64 * speed;
        let a = pos.floor() as isize;
        let f = (pos - a as f64) as f32;
        for ch in 0..2 {
            let x0 = sample(input, a, ch);
            let x1 = sample(input, a + 1, ch);
            out.push(x0 + (x1 - x0) * f);
        }
    }
    out
}

fn best_start(dec: &[f32], target: isize, nominal: isize, limit: isize) -> isize {
    let span = WINDOW / DECIMATE;
    let at = |i: isize| -> f32 {
        if i < 0 {
            0.0
        } else {
            dec.get(i as usize).copied().unwrap_or(0.0)
        }
    };
    let t0 = target / DECIMATE as isize;
    let mut best = nominal;
    let mut best_score = f32::MIN;
    let mut d = -SEARCH;
    while d <= SEARCH {
        let c = nominal + d;
        if c >= 0 && c <= limit {
            let c0 = c / DECIMATE as isize;
            let (mut dot, mut energy) = (0.0f32, 1e-6f32);
            for j in 0..span as isize {
                let v = at(c0 + j);
                dot += at(t0 + j) * v;
                energy += v * v;
            }
            let score = dot / energy.sqrt();
            if score > best_score {
                best_score = score;
                best = c;
            }
        }
        d += DECIMATE as isize;
    }
    best
}

pub fn stretch(input: &[f32], speed: f64) -> Vec<f32> {
    let frames = input.len() / 2;
    if (speed - 1.0).abs() < 1e-3 || frames == 0 {
        return input.to_vec();
    }
    if frames < WINDOW * 3 {
        return resample(input, speed);
    }
    let dec: Vec<f32> = (0..frames / DECIMATE)
        .map(|j| {
            let i = j * DECIMATE;
            (input[i * 2] + input[i * 2 + 1]) * 0.5
        })
        .collect();
    let win = hann();
    let out_frames = (frames as f64 / speed).ceil() as usize;
    let chunks = out_frames.div_ceil(HOP) + 1;
    let mut out = vec![0.0f32; (chunks * HOP + WINDOW) * 2];
    let limit = frames as isize - WINDOW as isize;
    let advance = HOP as f64 * speed;
    let mut prev: isize = 0;
    for k in 0..chunks {
        let nominal = ((k as f64 * advance).round() as isize).clamp(0, limit.max(0));
        let start = if k == 0 {
            nominal
        } else {
            best_start(&dec, prev + HOP as isize, nominal, limit)
        };
        prev = start;
        let base = k * HOP;
        for i in 0..WINDOW {
            let w = win[i];
            let src = start + i as isize;
            out[(base + i) * 2] += sample(input, src, 0) * w;
            out[(base + i) * 2 + 1] += sample(input, src, 1) * w;
        }
    }
    out.truncate(out_frames * 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, secs: f32) -> Vec<f32> {
        let n = (48_000.0 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / 48_000.0).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    fn crossings(buf: &[f32]) -> usize {
        buf.as_chunks::<2>()
            .0
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count()
    }

    #[test]
    fn unchanged_speed_returns_the_same_audio() {
        let a = tone(440.0, 0.5);
        assert_eq!(stretch(&a, 1.0), a);
    }

    #[test]
    fn faster_audio_is_shorter_and_slower_audio_is_longer() {
        let a = tone(440.0, 2.0);
        let fast = stretch(&a, 2.0);
        let slow = stretch(&a, 0.5);
        let n = a.len() as f64;
        assert!((fast.len() as f64 - n / 2.0).abs() < 8.0);
        assert!((slow.len() as f64 - n * 2.0).abs() < 8.0);
    }

    #[test]
    fn the_pitch_survives_a_speed_change() {
        let a = tone(440.0, 2.0);
        let rate = |buf: &[f32]| crossings(buf) as f64 / (buf.len() as f64 / 2.0 / 48_000.0);
        let base = rate(&a);
        for speed in [0.5, 1.5, 2.0] {
            let got = rate(&stretch(&a, speed));
            assert!(
                (got - base).abs() / base < 0.06,
                "speed {speed}: {got} vs {base}"
            );
        }
    }

    #[test]
    fn a_stretched_tone_keeps_its_loudness() {
        let a = tone(440.0, 2.0);
        let out = stretch(&a, 1.5);
        let mid = &out[out.len() / 4..out.len() * 3 / 4];
        let peak = mid.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.4 && peak < 0.6, "peak {peak}");
    }

    #[test]
    fn short_clips_fall_back_to_resampling() {
        let a = tone(440.0, 0.05);
        let out = stretch(&a, 2.0);
        assert!((out.len() as f64 - a.len() as f64 / 2.0).abs() < 4.0);
    }
}
