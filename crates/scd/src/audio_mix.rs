//! Keeps audio on the wall clock and mixes the microphone into desktop audio.
//!
//! Capture threads stamp each buffer on the shared clock, but the encoder times
//! packets by counting samples. Anything that breaks the one-to-one match
//! (pausing, a device dropout, the mic starting late) would slide audio against
//! video, so every stream goes through a [`Lane`] that pads gaps with silence
//! and trims overlaps, keeping sample count and timestamps in step.

use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::time::Timestamp;
use std::collections::VecDeque;

pub const RATE: i64 = 48_000;
pub const CHANNELS: usize = 2;
/// Gaps shorter than this are treated as jitter and left alone (10 ms).
const TOLERANCE: i64 = RATE / 100;
/// If one source runs this far ahead of the other, pad the other (500 ms).
const MAX_LEAD: i64 = RATE / 2;
/// Emit mixed audio in chunks of at least this many frames (20 ms).
const MIN_CHUNK: usize = (RATE / 50) as usize;

fn ns_to_frames(ns: i64) -> i64 {
    (ns as i128 * RATE as i128 / 1_000_000_000) as i64
}

fn frames_to_ns(frames: i64) -> i64 {
    (frames as i128 * 1_000_000_000 / RATE as i128) as i64
}

/// One audio stream's position on the timeline: how many frames it has produced
/// since the shared origin.
#[derive(Default)]
pub struct Lane {
    pub frames: i64,
}

impl Lane {
    /// Align a buffer starting at `ts_ns` to this lane. Returns the samples to
    /// append: zero-padded in front if the buffer starts later than the lane has
    /// reached, or with its first frames dropped if the lane is already past it.
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

    /// Zeros that bring the lane up to `frames`.
    fn pad_to(&mut self, frames: i64) -> Vec<f32> {
        if frames <= self.frames {
            return Vec::new();
        }
        let gap = (frames - self.frames) as usize;
        self.frames = frames;
        vec![0.0; gap * CHANNELS]
    }
}

/// Single stream (desktop-only or one separate track): aligned, with timestamps
/// that follow the sample count from the stream's own first buffer.
#[derive(Default)]
pub struct Track {
    origin_ns: Option<i64>,
    lane: Lane,
}

impl Track {
    /// Returns the aligned buffer to encode, if the input produced anything.
    pub fn push(&mut self, buf: &AudioBuffer) -> Option<AudioBuffer> {
        // Alignment math assumes the 48 kHz stereo the Windows capture produces;
        // other formats pass through untouched.
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

/// Desktop + microphone summed into one stream.
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

        // A source that hasn't started, or has stalled, must not hold the other
        // back: once one side leads by MAX_LEAD, fill the other with silence.
        if self.desktop.frames > self.mic.frames + MAX_LEAD {
            let pad = self.mic.pad_to(self.desktop.frames);
            self.mic_q.extend(pad);
        } else if self.mic.frames > self.desktop.frames + MAX_LEAD {
            let pad = self.desktop.pad_to(self.mic.frames);
            self.desktop_q.extend(pad);
        }
    }

    /// The next chunk of mixed audio, once both sides have enough.
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
    fn track_pads_a_gap_with_silence() {
        let mut t = Track::default();
        // 100 ms of audio, then the next buffer arrives stamped 300 ms later.
        let a = t.push(&buf(AudioRole::Desktop, 0, 4800, 0.5)).unwrap();
        assert_eq!(a.samples.len(), 4800 * 2);
        let b = t.push(&buf(AudioRole::Desktop, 300, 4800, 0.5)).unwrap();
        // 200 ms of silence (9600 frames) + the 100 ms buffer.
        assert_eq!(b.samples.len(), (9600 + 4800) * 2);
        assert_eq!(b.samples[0], 0.0);
        assert_eq!(*b.samples.last().unwrap(), 0.5);
        assert_eq!(b.timestamp.as_nanos(), 100 * 1_000_000);
    }

    #[test]
    fn track_trims_overlap() {
        let mut t = Track::default();
        t.push(&buf(AudioRole::Desktop, 0, 4800, 0.5)).unwrap();
        // Next buffer claims to start at 50 ms though 100 ms is already queued.
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
        // 1 s of desktop audio and no mic at all.
        for i in 0..10 {
            m.push(&buf(AudioRole::Desktop, i * 100, 4800, 0.25));
        }
        let mut total = 0;
        while let Some(b) = m.pop() {
            assert!((b.samples[0] - 0.25).abs() < 1e-6);
            total += b.samples.len() / CHANNELS;
        }
        // Everything beyond the 500 ms lead tolerance has been mixed out.
        assert!(total >= 24_000, "only {total} frames mixed");
    }

    #[test]
    fn late_mic_lines_up_by_timestamp() {
        let mut m = Mixer::default();
        for i in 0..10 {
            m.push(&buf(AudioRole::Desktop, i * 100, 4800, 0.25));
        }
        // The mic shows up 700 ms in, stamped on the same clock.
        m.push(&buf(AudioRole::Microphone, 700, 4800, 0.5));
        m.push(&buf(AudioRole::Desktop, 1000, 4800, 0.25));
        let mut all = Vec::new();
        while let Some(b) = m.pop() {
            all.extend(b.samples);
        }
        // Frame at 750 ms: desktop 0.25 + mic 0.5.
        let idx = (RATE as usize * 750 / 1000) * CHANNELS;
        assert!((all[idx] - 0.75).abs() < 1e-6, "got {}", all[idx]);
        // Frame at 500 ms: desktop only.
        let idx = (RATE as usize * 500 / 1000) * CHANNELS;
        assert!((all[idx] - 0.25).abs() < 1e-6);
    }
}
