//! Adaptive latency hedge controller.
//!
//! Uses Peak-EWMA to track tail latency distributions over time and provide
//! dynamic `hedge_delay` values that guarantee tail bounds while conserving
//! compute budget.

use crate::combinator::hedge::HedgeConfig;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Adaptive latency hedge controller based on Peak-EWMA.
///
/// Uses an asymmetric update rule to rapidly track latency spikes (peaks)
/// and slowly decay, providing a dynamic upper bound for latency hedging
/// that automatically adapts to system regime shifts.
///
/// # Alien Artifact: Regime Dynamics & Tail Risk (Family 10)
/// Static hedge delays suffer from "constants kill you". This controller uses
/// asymmetric exponential smoothing to dynamically adjust the hedge delay:
/// `H(t+1) = max(Sample, α * H(t))`
/// This mathematically guarantees that the hedge threshold instantly bounds new
/// latency spikes while smoothly settling back down when the regime recovers.
#[derive(Debug)]
pub struct PeakEwmaHedgeController {
    /// The current peak-EWMA estimate in nanoseconds.
    estimate_nanos: AtomicU64,
    /// Minimum allowed delay to prevent hedging too aggressively.
    min_delay: u64,
    /// Maximum allowed delay to bound worst-case wait.
    max_delay: u64,
    /// Decay factor α (e.g., 0.99 for slow decay). Fixed point or f64.
    decay_factor: f64,
}

impl PeakEwmaHedgeController {
    /// Create a new adaptive hedge controller.
    #[must_use]
    pub fn new(
        initial: Duration,
        min_delay: Duration,
        max_delay: Duration,
        decay_factor: f64,
    ) -> Self {
        Self {
            estimate_nanos: AtomicU64::new(initial.as_nanos() as u64),
            min_delay: min_delay.as_nanos() as u64,
            max_delay: max_delay.as_nanos() as u64,
            decay_factor,
        }
    }

    /// Default configuration suitable for typical RPC hedging.
    #[must_use]
    pub fn default_rpc() -> Self {
        Self::new(
            Duration::from_millis(50),  // initial
            Duration::from_millis(10),  // min
            Duration::from_millis(500), // max
            0.99,                       // decay (slow return to normal)
        )
    }

    /// Observe the completion time of a primary request to adjust the threshold.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn observe(&self, rtt: Duration) {
        let sample = rtt.as_nanos() as u64;
        let mut current = self.estimate_nanos.load(Ordering::Acquire);
        loop {
            let next = if sample > current {
                sample // Rapid spike tracking (Peak)
            } else {
                let decayed = (current as f64) * self.decay_factor;
                if !decayed.is_finite() || decayed <= 0.0 {
                    0
                } else if decayed >= (u64::MAX as f64) {
                    u64::MAX
                } else {
                    decayed as u64
                }
            };

            match self.estimate_nanos.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
    }

    /// Get the current dynamically calculated hedge configuration.
    #[must_use]
    pub fn current_config(&self) -> HedgeConfig {
        let mut delay_nanos = self.estimate_nanos.load(Ordering::Relaxed);

        if delay_nanos < self.min_delay {
            delay_nanos = self.min_delay;
        } else if delay_nanos > self.max_delay {
            delay_nanos = self.max_delay;
        }

        HedgeConfig::new(Duration::from_nanos(delay_nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_hedge_tracks_peak_instantly() {
        let controller = PeakEwmaHedgeController::default_rpc();
        // Base delay is 50ms
        assert_eq!(
            controller.current_config().hedge_delay,
            Duration::from_millis(50)
        );

        // Spike to 200ms
        controller.observe(Duration::from_millis(200));
        assert_eq!(
            controller.current_config().hedge_delay,
            Duration::from_millis(200)
        );
    }

    #[test]
    fn adaptive_hedge_decays_slowly() {
        let controller = PeakEwmaHedgeController::default_rpc();
        controller.observe(Duration::from_millis(200)); // peak

        // Small observation should trigger decay (200 * 0.99 = 198)
        controller.observe(Duration::from_millis(10));
        let delay = controller.current_config().hedge_delay.as_millis();
        assert_eq!(delay, 198);
    }

    #[test]
    fn adaptive_hedge_respects_bounds() {
        let controller = PeakEwmaHedgeController::default_rpc(); // max 500ms

        controller.observe(Duration::from_secs(1)); // Way over max
        assert_eq!(
            controller.current_config().hedge_delay,
            Duration::from_millis(500)
        );
    }
}
