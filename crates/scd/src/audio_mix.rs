use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::time::Timestamp;
use std::collections::VecDeque;

pub const RATE: i64 = 48_000;
pub const CHANNELS: usize = 2;
const TOLERANCE: i64 = RATE / 100;
const MAX_LEAD: i64 = RATE / 2;
const MIN_CHUNK: usize = (RATE / 50) as usize;

fn ns_to_frames(ns: i64) -> i64 {
    (ns as i128 * RATE as i128 / 1_000_000_000) as i64
}

fn frames_to_ns(frames: i64) -> i64 {
    (frames as i128 * 1_000_000_000 / RATE as i128) as i64
}

pub fn apply_volume(samples: &mut [f32], percent: u32) {
    if percent == 100 {
        return;
    }
    let gain = percent as f32 / 100.0;
    for s in samples {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

#[derive(Default)]
pub struct Lane {
    pub frames: i64,
}

impl Lane {
    pub fn align(&mut self, origin_ns: i64, ts_ns: i64, samples: &[f32]) -> Vec<f32> {
        let expected = ns_to_frames(ts_ns - origin_ns).max(0);
        let mut out = Vec::new();
        let mut body = samples;
        if expected > self.frames + TOLERANCE {
            let gap = (expected - self.frames) as usize;
            out.resize(gap * CHANNELS, 0.0);
            self.frames += gap as i64;
        } else if expected + TOLERANCE < self.frames {
            let skip = ((self.frames - expected) as usize * CHANNELS).min(body.len());
            body = &body[skip..];
        }
        out.extend_from_slice(body);
        self.frames += (body.len() / CHANNELS) as i64;
        out
    }

    fn pad_to(&mut self, frames: i64) -> Vec<f32> {
        if frames <= self.frames {
            return Vec::new();
        }
        let gap = (frames - self.frames) as usize;
        self.frames = frames;
        vec![0.0; gap * CHANNELS]
    }
}

#[derive(Default)]
pub struct Track {
    origin_ns: Option<i64>,
    lane: Lane,
}

impl Track {
    pub fn push(&mut self, buf: &AudioBuffer) -> Option<AudioBuffer> {
        if buf.sample_rate != RATE as u32 || buf.channels != CHANNELS as u16 {
            return Some(AudioBuffer {
                role: buf.role,
                sample_rate: buf.sample_rate,
                channels: buf.channels,
                timestamp: buf.timestamp,
                samples: buf.samples.clone(),
            });
        }
        let origin = *self.origin_ns.get_or_insert(buf.timestamp.as_nanos());
        let before = self.lane.frames;
        let samples = self
            .lane
            .align(origin, buf.timestamp.as_nanos(), &buf.samples);
        if samples.is_empty() {
            return None;
        }
        Some(AudioBuffer {
            role: buf.role,
            sample_rate: RATE as u32,
            channels: CHANNELS as u16,
            timestamp: Timestamp::from_nanos(origin + frames_to_ns(before)),
            samples,
        })
    }
}

#[derive(Default)]
pub struct Mixer {
    origin_ns: Option<i64>,
    desktop: Lane,
    mic: Lane,
    desktop_q: VecDeque<f32>,
    mic_q: VecDeque<f32>,
    emitted: i64,
}

impl Mixer {
    pub fn push(&mut self, buf: &AudioBuffer) {
        let origin = *self.origin_ns.get_or_insert(buf.timestamp.as_nanos());
        let (lane, q) = match buf.role {
            AudioRole::Desktop => (&mut self.desktop, &mut self.desktop_q),
            AudioRole::Microphone => (&mut self.mic, &mut self.mic_q),
        };
        q.extend(lane.align(origin, buf.timestamp.as_nanos(), &buf.samples));

        if self.desktop.frames > self.mic.frames + MAX_LEAD {
            let pad = self.mic.pad_to(self.desktop.frames);
            self.mic_q.extend(pad);
        } else if self.mic.frames > self.desktop.frames + MAX_LEAD {
            let pad = self.desktop.pad_to(self.mic.frames);
            self.desktop_q.extend(pad);
        }
    }

    pub fn pop(&mut self) -> Option<AudioBuffer> {
        let n = self.desktop_q.len().min(self.mic_q.len()) / CHANNELS;
        if n < MIN_CHUNK {
            return None;
        }
        let mut samples = Vec::with_capacity(n * CHANNELS);
        for _ in 0..n * CHANNELS {
            let a = self.desktop_q.pop_front().unwrap_or(0.0);
            let b = self.mic_q.pop_front().unwrap_or(0.0);
            samples.push((a + b).clamp(-1.0, 1.0));
        }
        let ts = self.origin_ns.unwrap_or(0) + frames_to_ns(self.emitted);
        self.emitted += n as i64;
        Some(AudioBuffer {
            role: AudioRole::Desktop,
            sample_rate: RATE as u32,
            channels: CHANNELS as u16,
            timestamp: Timestamp::from_nanos(ts),
            samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(role: AudioRole, ts_ms: i64, frames: usize, v: f32) -> AudioBuffer {
        AudioBuffer {
            role,
            sample_rate: RATE as u32,
            channels: CHANNELS as u16,
            timestamp: Timestamp::from_nanos(ts_ms * 1_000_000),
            samples: vec![v; frames * CHANNELS],
        }
    }

    #[test]
    fn volume_scales_and_clips() {
        let mut s = vec![0.5, -0.5, 0.8];
        apply_volume(&mut s, 100);
        assert_eq!(s, vec![0.5, -0.5, 0.8]);
        apply_volume(&mut s, 50);
        assert_eq!(s, vec![0.25, -0.25, 0.4]);
        apply_volume(&mut s, 200);
        assert_eq!(s, vec![0.5, -0.5, 0.8]);
        apply_volume(&mut s, 200);
        assert_eq!(s[2], 1.0);
        apply_volume(&mut s, 0);
        assert!(s.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn track_pads_a_gap_with_silence() {
        let mut t = Track::default();
        let a = t.push(&buf(AudioRole::Desktop, 0, 4800, 0.5)).unwrap();
        assert_eq!(a.samples.len(), 4800 * 2);
        let b = t.push(&buf(AudioRole::Desktop, 300, 4800, 0.5)).unwrap();
        assert_eq!(b.samples.len(), (9600 + 4800) * 2);
        assert_eq!(b.samples[0], 0.0);
        assert_eq!(*b.samples.last().unwrap(), 0.5);
        assert_eq!(b.timestamp.as_nanos(), 100 * 1_000_000);
    }

    #[test]
    fn track_trims_overlap() {
        let mut t = Track::default();
        t.push(&buf(AudioRole::Desktop, 0, 4800, 0.5)).unwrap();
        let b = t.push(&buf(AudioRole::Desktop, 50, 4800, 0.5)).unwrap();
        assert_eq!(b.samples.len(), (4800 - 2400) * 2);
    }

    #[test]
    fn mixer_sums_both_sides() {
        let mut m = Mixer::default();
        m.push(&buf(AudioRole::Desktop, 0, 4800, 0.25));
        m.push(&buf(AudioRole::Microphone, 0, 4800, 0.5));
        let out = m.pop().unwrap();
        assert_eq!(out.samples.len(), 4800 * 2);
        assert!((out.samples[0] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn mixer_clips_instead_of_wrapping() {
        let mut m = Mixer::default();
        m.push(&buf(AudioRole::Desktop, 0, 4800, 0.9));
        m.push(&buf(AudioRole::Microphone, 0, 4800, 0.9));
        assert_eq!(m.pop().unwrap().samples[0], 1.0);
    }

    #[test]
    fn missing_mic_does_not_stall_desktop() {
        let mut m = Mixer::default();
        for i in 0..10 {
            m.push(&buf(AudioRole::Desktop, i * 100, 4800, 0.25));
        }
        let mut total = 0;
        while let Some(b) = m.pop() {
            assert!((b.samples[0] - 0.25).abs() < 1e-6);
            total += b.samples.len() / CHANNELS;
        }
        assert!(total >= 24_000, "only {total} frames mixed");
    }

    #[test]
    fn late_mic_lines_up_by_timestamp() {
        let mut m = Mixer::default();
        for i in 0..10 {
            m.push(&buf(AudioRole::Desktop, i * 100, 4800, 0.25));
        }
        m.push(&buf(AudioRole::Microphone, 700, 4800, 0.5));
        m.push(&buf(AudioRole::Desktop, 1000, 4800, 0.25));
        let mut all = Vec::new();
        while let Some(b) = m.pop() {
            all.extend(b.samples);
        }
        let idx = (RATE as usize * 750 / 1000) * CHANNELS;
        assert!((all[idx] - 0.75).abs() < 1e-6, "got {}", all[idx]);
        let idx = (RATE as usize * 500 / 1000) * CHANNELS;
        assert!((all[idx] - 0.25).abs() < 1e-6);
    }
}
