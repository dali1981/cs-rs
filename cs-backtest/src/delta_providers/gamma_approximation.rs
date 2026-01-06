//! Gamma approximation delta provider
//!
//! Provides incremental delta updates using the gamma approximation formula:
//! δ' = δ + γ × (S' - S)
//!
//! This is the fastest method as it avoids Black-Scholes computations,
//! but assumes constant gamma which becomes less accurate for large spot moves.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cs_domain::hedging::DeltaProvider;

/// Incremental delta using gamma approximation
///
/// This provider matches the current HedgeState behavior - it maintains
/// delta and gamma state and updates delta incrementally as spot moves.
///
/// # Delta Convention
/// Returns per-share delta (e.g., 0.5 for ATM call, NOT 50)
pub struct GammaApproximationProvider {
    option_delta: f64,      // Per-share delta
    option_gamma: f64,      // Per-share gamma
    last_spot: f64,
}

impl GammaApproximationProvider {
    /// Create new provider with initial greeks
    ///
    /// # Arguments
    /// * `initial_delta` - Per-share option delta at entry
    /// * `initial_gamma` - Per-share option gamma at entry
    /// * `initial_spot` - Spot price at entry
    pub fn new(initial_delta: f64, initial_gamma: f64, initial_spot: f64) -> Self {
        Self {
            option_delta: initial_delta,
            option_gamma: initial_gamma,
            last_spot: initial_spot,
        }
    }
}

#[async_trait]
impl DeltaProvider for GammaApproximationProvider {
    async fn compute_delta(&mut self, spot: f64, _timestamp: DateTime<Utc>) -> Result<f64, String> {
        // Incremental update: δ' = δ + γ × (S' - S)
        let spot_change = spot - self.last_spot;
        self.option_delta += self.option_gamma * spot_change;
        self.last_spot = spot;

        Ok(self.option_delta)  // Per-share, NO multiplier
    }

    fn compute_gamma(&self, _spot: f64, _timestamp: DateTime<Utc>) -> Option<f64> {
        Some(self.option_gamma)
    }

    fn name(&self) -> &'static str {
        "gamma_approximation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_incremental_delta_update() {
        let mut provider = GammaApproximationProvider::new(
            0.5,   // Delta = 0.5 (ATM call)
            0.02,  // Gamma = 0.02
            100.0, // Spot = $100
        );

        // Spot moves up to $101
        let delta = provider.compute_delta(101.0, Utc::now()).await.unwrap();
        // Expected: 0.5 + 0.02 * (101 - 100) = 0.5 + 0.02 = 0.52
        assert!((delta - 0.52).abs() < 1e-10);

        // Spot moves down to $99
        let delta = provider.compute_delta(99.0, Utc::now()).await.unwrap();
        // Expected: 0.52 + 0.02 * (99 - 101) = 0.52 - 0.04 = 0.48
        assert!((delta - 0.48).abs() < 1e-10);
    }

    #[tokio::test]
    async fn test_gamma_available() {
        let provider = GammaApproximationProvider::new(0.5, 0.02, 100.0);

        let gamma = provider.compute_gamma(100.0, Utc::now());
        assert_eq!(gamma, Some(0.02));
    }

    #[tokio::test]
    async fn test_per_share_convention() {
        let mut provider = GammaApproximationProvider::new(0.5, 0.02, 100.0);

        let delta = provider.compute_delta(100.0, Utc::now()).await.unwrap();

        // Delta should be per-share (0.5), NOT multiplied by 100
        assert!(delta.abs() < 2.0, "Delta should be per-share, got {}", delta);
    }
}
