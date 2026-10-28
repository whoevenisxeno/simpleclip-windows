use crate::encode::EncodedPacket;
use crate::time::Timestamp;
use std::collections::VecDeque;

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

    pub fn latest(&self) -> Option<Timestamp> {
        self.packets.back().map(|p| p.timestamp)
    }
}

impl PacketRing {
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

    pub fn snapshot_since(&self, since: i64) -> Vec<EncodedPacket> {
        self.packets
            .iter()
            .filter(|p| p.timestamp.as_nanos() >= since)
            .cloned()
            .collect()
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
