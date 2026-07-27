use crate::encode::EncodedPacket;
use crate::time::Timestamp;
use std::collections::VecDeque;

/// A byte-capped ring of encoded packets for one stream. Eviction removes whole
/// leading GOPs so the buffer always begins on a keyframe, which is what lets a
/// saved clip start cleanly (§6.2). One ring per track (video, desktop, mic).
pub struct PacketRing {
    packets: VecDeque<EncodedPacket>,
    bytes: usize,
    max_bytes: usize,
}

impl PacketRing {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            packets: VecDeque::new(),
            bytes: 0,
            max_bytes,
        }
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Fraction of the byte budget currently used, 0.0..=1.0.
    pub fn fill(&self) -> f32 {
        if self.max_bytes == 0 {
            return 0.0;
        }
        (self.bytes as f32 / self.max_bytes as f32).min(1.0)
    }
}

impl PacketRing {
    pub fn push(&mut self, packet: EncodedPacket) {
        self.bytes += packet.data.len();
        self.packets.push_back(packet);
        self.evict();
    }

    /// Drop whole leading GOPs while over budget. Never drops a partial GOP and
    /// never removes the last remaining GOP, so the front stays a keyframe and
    /// the ring is never emptied below one clip-startable GOP.
    fn evict(&mut self) {
        while self.bytes > self.max_bytes {
            let second_gop = self
                .packets
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, p)| p.keyframe)
                .map(|(i, _)| i);
            let Some(cut) = second_gop else { break };
            for _ in 0..cut {
                let p = self.packets.pop_front().unwrap();
                self.bytes -= p.data.len();
            }
        }
    }

    /// Newest capture timestamp in the ring, if any.
    pub fn latest(&self) -> Option<Timestamp> {
        self.packets.back().map(|p| p.timestamp)
    }
}

impl PacketRing {
    /// Clone the last `secs` seconds of packets, starting at the latest keyframe
    /// at or before the window start so playback begins cleanly. Cheap-ish: this
    /// clones packet bytes, but the caller does it off the capture thread (§6.3).
    pub fn snapshot(&self, secs: u32) -> Vec<EncodedPacket> {
        let Some(latest) = self.latest() else {
            return Vec::new();
        };
        let window_start = latest.as_nanos() - (secs as i64) * 1_000_000_000;
        let mut start_idx = 0;
        for (i, p) in self.packets.iter().enumerate() {
            if p.keyframe && p.timestamp.as_nanos() <= window_start {
                start_idx = i;
            }
            if p.timestamp.as_nanos() > window_start {
                break;
            }
        }
        self.packets.iter().skip(start_idx).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(ts_ms: i64, key: bool, size: usize) -> EncodedPacket {
        EncodedPacket {
            timestamp: Timestamp::from_nanos(ts_ms * 1_000_000),
            duration: 16,
            keyframe: key,
            data: vec![0u8; size],
        }
    }

    #[test]
    fn front_is_always_keyframe_after_eviction() {
        let mut ring = PacketRing::new(300);
        for i in 0..40 {
            ring.push(pkt(i * 100, i % 4 == 0, 100));
        }
        assert!(!ring.is_empty());
        assert!(ring.packets.front().unwrap().keyframe);
        // Bounded: budget plus at most one extra in-progress GOP.
        assert!(ring.bytes() <= 300 + 4 * 100);
    }

    #[test]
    fn snapshot_starts_on_keyframe() {
        let mut ring = PacketRing::new(1_000_000);
        for i in 0..40 {
            ring.push(pkt(i * 100, i % 4 == 0, 10));
        }
        let snap = ring.snapshot(1);
        assert!(!snap.is_empty());
        assert!(snap.first().unwrap().keyframe);
    }
}
