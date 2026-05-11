//! Time utilities for seriousum.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy)]
pub struct Timer {
    start: Instant,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    pub fn reset(&mut self) {
        self.start = Instant::now();
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    instant: Instant,
}

impl Deadline {
    pub fn new(duration: Duration) -> Self {
        Self {
            instant: Instant::now() + duration,
        }
    }
    pub fn remaining(&self) -> Option<Duration> {
        self.instant.checked_duration_since(Instant::now())
    }
    pub fn has_passed(&self) -> bool {
        Instant::now() >= self.instant
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Timestamp {
    pub nanos: i64,
}

impl Timestamp {
    pub fn new(nanos: i64) -> Self {
        Self { nanos }
    }
    pub fn now() -> Self {
        Self {
            nanos: Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        }
    }
    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self {
            nanos: dt.timestamp_nanos_opt().unwrap_or_default(),
        }
    }
    pub fn as_datetime(&self) -> Option<DateTime<Utc>> {
        let secs = self.nanos.div_euclid(1_000_000_000);
        let nanos = self.nanos.rem_euclid(1_000_000_000) as u32;
        DateTime::<Utc>::from_timestamp(secs, nanos)
    }
}

impl Default for Timestamp {
    fn default() -> Self {
        Self::now()
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.as_datetime() {
            Some(dt) => write!(f, "{}", dt.to_rfc3339()),
            None => write!(f, "{}", self.nanos),
        }
    }
}

#[derive(Debug)]
pub struct RateLimiter {
    max_events: usize,
    interval: Duration,
    current_count: usize,
    last_reset: Instant,
}

impl RateLimiter {
    pub fn new(max_events: usize, interval: Duration) -> Self {
        Self {
            max_events,
            interval,
            current_count: 0,
            last_reset: Instant::now(),
        }
    }
    pub fn allow(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_reset) >= self.interval {
            self.current_count = 0;
            self.last_reset = now;
        }
        if self.current_count < self.max_events {
            self.current_count += 1;
            true
        } else {
            false
        }
    }
    pub fn remaining(&self) -> usize {
        if Instant::now().duration_since(self.last_reset) >= self.interval {
            self.max_events
        } else {
            self.max_events.saturating_sub(self.current_count)
        }
    }
}
