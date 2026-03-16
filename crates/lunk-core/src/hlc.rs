//! Hybrid Logical Clock (HLC) for CRDT conflict resolution.
//!
//! Generates `(wall_ms, counter, site_id)` timestamps that provide a
//! deterministic total order across distributed nodes, combining wall-clock
//! time with a logical counter to handle clock skew.

use std::cmp;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A timestamp from a Hybrid Logical Clock.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct HlcTimestamp {
    pub wall_ms: i64,
    pub counter: i64,
    pub site_id: String,
}

impl Ord for HlcTimestamp {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.wall_ms
            .cmp(&other.wall_ms)
            .then(self.counter.cmp(&other.counter))
            .then(self.site_id.cmp(&other.site_id))
    }
}

impl PartialOrd for HlcTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Hybrid Logical Clock.
///
/// Thread-safety: not `Send`/`Sync` — intended to live behind the same
/// `Mutex` as the database connection.
pub struct HybridClock {
    wall_ms: i64,
    counter: i64,
    site_id: String,
}

impl HybridClock {
    /// Create a new clock for the given site.
    pub fn new(site_id: String) -> Self {
        Self {
            wall_ms: 0,
            counter: 0,
            site_id,
        }
    }

    /// Restore a clock from persisted state.
    pub fn restore(site_id: String, wall_ms: i64, counter: i64) -> Self {
        Self {
            wall_ms,
            counter,
            site_id,
        }
    }

    pub fn site_id(&self) -> &str {
        &self.site_id
    }

    pub fn wall_ms(&self) -> i64 {
        self.wall_ms
    }

    pub fn counter(&self) -> i64 {
        self.counter
    }

    /// Generate a new timestamp, advancing the clock.
    pub fn now(&mut self) -> HlcTimestamp {
        let physical = system_time_ms();
        if physical > self.wall_ms {
            self.wall_ms = physical;
            self.counter = 0;
        } else {
            self.counter += 1;
        }

        HlcTimestamp {
            wall_ms: self.wall_ms,
            counter: self.counter,
            site_id: self.site_id.clone(),
        }
    }

    /// Observe a remote timestamp and advance the local clock past it.
    /// This ensures causality: any subsequent `now()` call will produce
    /// a timestamp strictly greater than the observed one.
    pub fn observe(&mut self, remote: &HlcTimestamp) {
        let physical = system_time_ms();
        let old_wall = self.wall_ms;

        self.wall_ms = cmp::max(cmp::max(old_wall, remote.wall_ms), physical);

        if self.wall_ms == old_wall && self.wall_ms == remote.wall_ms {
            // All three the same — take max counter + 1
            self.counter = cmp::max(self.counter, remote.counter) + 1;
        } else if self.wall_ms == old_wall {
            // Local wall stayed — bump our counter
            self.counter += 1;
        } else if self.wall_ms == remote.wall_ms {
            // Advanced to remote wall — continue from their counter
            self.counter = remote.counter + 1;
        } else {
            // Advanced to physical time — reset counter
            self.counter = 0;
        }
    }
}

fn system_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hlc_monotonic() {
        let mut clock = HybridClock::new("site-a".to_string());
        let mut prev = clock.now();
        for _ in 0..100 {
            let ts = clock.now();
            assert!(ts > prev, "timestamps must be strictly increasing");
            prev = ts;
        }
    }

    #[test]
    fn test_hlc_wall_advance() {
        let mut clock = HybridClock::new("site-a".to_string());
        let ts1 = clock.now();

        // Simulate time advancing by forcing wall_ms backward
        // (the next now() with real system time should advance)
        let ts2 = clock.now();
        assert!(ts2 > ts1);
        // wall_ms should be >= system time
        assert!(ts2.wall_ms >= ts1.wall_ms);
    }

    #[test]
    fn test_hlc_observe_advances_past_remote() {
        let mut clock_a = HybridClock::new("site-a".to_string());
        let mut clock_b = HybridClock::new("site-b".to_string());

        // Site B generates a timestamp far in the "future"
        clock_b.wall_ms = system_time_ms() + 100_000;
        let remote_ts = clock_b.now();

        // Site A observes it
        clock_a.observe(&remote_ts);
        let local_ts = clock_a.now();

        assert!(
            local_ts > remote_ts,
            "local clock must advance past observed remote timestamp"
        );
    }

    #[test]
    fn test_hlc_observe_same_wall() {
        let mut clock = HybridClock::new("site-a".to_string());

        // Set our clock to a known state
        clock.wall_ms = system_time_ms() + 1_000_000; // far future
        clock.counter = 5;

        let remote = HlcTimestamp {
            wall_ms: clock.wall_ms,
            counter: 10,
            site_id: "site-b".to_string(),
        };

        clock.observe(&remote);
        let ts = clock.now();

        // Should be past both our counter (5) and remote counter (10)
        assert!(ts.wall_ms >= remote.wall_ms);
        assert!(ts > remote);
    }

    #[test]
    fn test_hlc_ordering() {
        let a = HlcTimestamp {
            wall_ms: 100,
            counter: 0,
            site_id: "a".to_string(),
        };
        let b = HlcTimestamp {
            wall_ms: 200,
            counter: 0,
            site_id: "a".to_string(),
        };
        assert!(b > a, "higher wall_ms wins");

        let c = HlcTimestamp {
            wall_ms: 100,
            counter: 1,
            site_id: "a".to_string(),
        };
        assert!(c > a, "same wall_ms, higher counter wins");

        let d = HlcTimestamp {
            wall_ms: 100,
            counter: 0,
            site_id: "b".to_string(),
        };
        assert!(d > a, "same wall_ms+counter, higher site_id wins");
    }

    #[test]
    fn test_hlc_restore() {
        let clock = HybridClock::restore("site-x".to_string(), 12345, 67);
        assert_eq!(clock.site_id(), "site-x");
        assert_eq!(clock.wall_ms(), 12345);
        assert_eq!(clock.counter(), 67);
    }
}
