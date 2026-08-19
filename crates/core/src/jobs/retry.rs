// SPDX-License-Identifier: AGPL-3.0-only

use std::time::Duration;

pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;
pub const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(1_000);
pub const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(60);
pub const DEFAULT_JITTER_RATIO: f64 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retryability {
    Retryable,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    /// Fraction of the backoff that may be added or subtracted. `0.2` means
    /// ±20%. Must be in `0.0..=1.0`.
    pub jitter_ratio: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
            jitter_ratio: DEFAULT_JITTER_RATIO,
        }
    }
}

impl RetryPolicy {
    pub fn new(
        max_attempts: u32,
        initial_backoff: Duration,
        max_backoff: Duration,
        jitter_ratio: f64,
    ) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            initial_backoff,
            max_backoff: max_backoff.max(initial_backoff),
            jitter_ratio: jitter_ratio.clamp(0.0, 1.0),
        }
    }

    pub fn can_retry(self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }

    /// Backoff after `attempt` (1-based) has failed.
    ///
    /// `jitter_unit` is a sample in `0.0..=1.0`. `0.5` applies no jitter.
    pub fn backoff(self, attempt: u32, jitter_unit: f64) -> Duration {
        let exp_index = attempt.saturating_sub(1).min(16);
        let factor = 2u32.saturating_pow(exp_index) as u128;
        let initial_ms = self.initial_backoff.as_millis();
        let max_ms = self.max_backoff.as_millis();
        let capped = (initial_ms.saturating_mul(factor)).min(max_ms);

        let unit = jitter_unit.clamp(0.0, 1.0);
        let signed = (unit * 2.0 - 1.0) * self.jitter_ratio;
        let jittered = (capped as f64 * (1.0 + signed)).max(0.0);
        Duration::from_millis(jittered as u64)
    }

    pub fn sampled_backoff(self, attempt: u32) -> Duration {
        self.backoff(attempt, rand::random::<f64>())
    }
}
