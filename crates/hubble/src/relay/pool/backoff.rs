//! Exponential backoff strategy for connection retries

use std::time::Duration;

/// Exponential backoff for retry logic
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    /// Minimum backoff duration
    pub min: Duration,
    /// Maximum backoff duration
    pub max: Duration,
    /// Backoff multiplier factor
    pub factor: f64,
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        #[allow(clippy::duration_suboptimal_units)]
        Self {
            min: Duration::from_secs(1),
            max: Duration::from_secs(60),
            factor: 2.0,
        }
    }
}

impl ExponentialBackoff {
    /// Creates a new exponential backoff with custom parameters
    pub fn new(min: Duration, max: Duration, factor: f64) -> Self {
        Self { min, max, factor }
    }

    /// Calculates the duration for the given attempt number (0-based)
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_wrap)]
    pub fn duration(&self, attempt: usize) -> Duration {
        let exp = (self.factor).powi(attempt.min(62) as i32);
        let millis = (self.min.as_millis() as f64 * exp).min(u128::MAX as f64) as u128;
        let result = Duration::from_millis(millis as u64);

        // Cap at maximum
        if result > self.max { self.max } else { result }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_backoff_defaults() {
        let backoff = ExponentialBackoff::default();
        assert_eq!(backoff.min, Duration::from_secs(1));
        assert_eq!(backoff.max, Duration::from_secs(60));
        assert_eq!(backoff.factor, 2.0);
    }

    #[test]
    fn exponential_backoff_progression() {
        let backoff = ExponentialBackoff::default();

        // First attempt: 1s
        assert_eq!(backoff.duration(0), Duration::from_secs(1));
        // Second attempt: 2s
        assert_eq!(backoff.duration(1), Duration::from_secs(2));
        // Third attempt: 4s
        assert_eq!(backoff.duration(2), Duration::from_secs(4));
        // Fourth attempt: 8s
        assert_eq!(backoff.duration(3), Duration::from_secs(8));
    }

    #[test]
    fn exponential_backoff_caps_at_max() {
        let backoff = ExponentialBackoff::default();

        // Even with many attempts, should not exceed max
        for attempt in 10..20 {
            let duration = backoff.duration(attempt);
            assert!(duration <= backoff.max);
        }
    }

    #[test]
    fn exponential_backoff_custom() {
        let backoff =
            ExponentialBackoff::new(Duration::from_millis(100), Duration::from_secs(10), 3.0);

        assert_eq!(backoff.duration(0), Duration::from_millis(100));
        assert_eq!(backoff.duration(1), Duration::from_millis(300));
    }
}
