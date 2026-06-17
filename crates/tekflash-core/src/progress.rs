//! Cross-cutting progress events.
//!
//! Stages emit `Event` records onto a broadcast channel; the TUI subscribes and renders
//! gauges, the JSON mode subscribes and prints NDJSON. Keeping it event-based means we
//! don't pay UI rendering cost in scriptable mode and vice versa.

use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Started {
        operation: &'static str,
        total_bytes: Option<u64>,
    },
    Progress {
        stage: &'static str,
        bytes_in: u64,
        bytes_out: u64,
    },
    Finished {
        bytes_in: u64,
        bytes_out: u64,
        hash_hex: Option<String>,
    },
    VerifyStarted {
        total_bytes: u64,
        mode: VerifyMode,
    },
    VerifyProgress {
        bytes_read: u64,
    },
    VerifyResult {
        passed: bool,
        mismatched_offset: Option<u64>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerifyMode {
    Off,
    Full,
    Sampled,
    Deferred,
}

/// Convenience timer for rate / ETA computation.
#[derive(Debug)]
pub struct RateMeter {
    started: Instant,
    last: Instant,
    last_bytes: u64,
}

impl RateMeter {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            last_bytes: 0,
        }
    }

    /// Returns (instant rate B/s, average rate B/s).
    pub fn tick(&mut self, current_bytes: u64) -> (f64, f64) {
        let now = Instant::now();
        let dt_inst = now
            .saturating_duration_since(self.last)
            .as_secs_f64()
            .max(1e-3);
        let dt_avg = now
            .saturating_duration_since(self.started)
            .as_secs_f64()
            .max(1e-3);
        let inst = (current_bytes - self.last_bytes) as f64 / dt_inst;
        let avg = current_bytes as f64 / dt_avg;
        self.last = now;
        self.last_bytes = current_bytes;
        (inst, avg)
    }
}

impl Default for RateMeter {
    fn default() -> Self {
        Self::new()
    }
}
