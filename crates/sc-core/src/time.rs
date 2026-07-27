use serde::{Deserialize, Serialize};

/// Monotonic capture timestamp in nanoseconds, shared across audio and video
/// sources so the muxer can align tracks. Never derived from wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub const fn from_nanos(ns: i64) -> Self {
        Self(ns)
    }

    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    pub fn as_millis(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }
}

impl std::ops::Sub for Timestamp {
    type Output = i64;
    fn sub(self, rhs: Self) -> i64 {
        self.0 - rhs.0
    }
}
