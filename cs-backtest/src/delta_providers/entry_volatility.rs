//! Entry volatility delta provider
//!
//! Recomputes delta from Black-Scholes using fixed volatility from trade entry.
//! This provider is shared by both EntryHV and EntryIV modes - they differ only
//! in where the volatility value comes from (historical vol vs implied vol).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cs_domain::hedging::DeltaProvider;
use cs_domain::trade::CompositeTrade;
use super::common::compute_position_delta_uniform_vol;

/// Recompute delta from Black-Scholes using fixed volatility
///
/// Used for both EntryHV and EntryIV modes - they differ only
/// in where the volatility value comes from.
///
/// # Delta Convention
/// Returns per-share delta (e.g., 0.5 for ATM call, NOT 50)
pub struct EntryVolatilityProvider<T: CompositeTrade> {
    trade: T,
    entry_volatility: f64,      // Fixed vol (HV or IV at entry)
    risk_free_rate: f64,
    vol_source_name: &'static str,  // "entry_hv" or "entry_iv"
}

impl<T: CompositeTrade> EntryVolatilityProvider<T> {
    /// Create provider using entry historical volatility
    pub fn new_entry_hv(trade: T, entry_hv: f64, risk_free_rate: f64) -> Self {
        Self {
            trade,
            entry_volatility: entry_hv,
            risk_free_rate,
            vol_source_name: "entry_hv",
        }
    }

    /// Create provider using entry implied volatility
    pub fn new_entry_iv(trade: T, entry_iv: f64, risk_free_rate: f64) -> Self {
        Self {
            trade,
            entry_volatility: entry_iv,
            risk_free_rate,
            vol_source_name: "entry_iv",
        }
    }
}

#[async_trait]
impl<T: CompositeTrade + Send + Sync> DeltaProvider for EntryVolatilityProvider<T> {
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
        let position_delta = compute_position_delta_uniform_vol(
            &self.trade,
            spot,
            timestamp,
            self.entry_volatility,
            self.risk_free_rate,
        );
        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        self.vol_source_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_domain::entities::OptionLeg;
    use cs_domain::trade::LegPosition;
    use cs_domain::value_objects::Strike;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    // Simple test trade implementation
    #[derive(Clone)]
    struct TestTrade {
        legs: Vec<(OptionLeg, LegPosition)>,
    }

    impl CompositeTrade for TestTrade {
        fn legs(&self) -> Vec<(&OptionLeg, LegPosition)> {
            self.legs.iter().map(|(leg, pos)| (leg, *pos)).collect()
        }
    }

    #[tokio::test]
    async fn test_single_call_delta() {
        let expiration = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let leg = OptionLeg::new(
            "SPY".to_string(),
            Strike::new(Decimal::from(100)).unwrap(),
            expiration,
            OptionType::Call,
        );
        let position = LegPosition::Long;

        let trade = TestTrade {
            legs: vec![(leg, position)],
        };

        let mut provider = EntryVolatilityProvider::new_entry_hv(trade, 0.20, 0.05);

        // Compute delta at entry (ATM should be ~0.5)
        let entry_time = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            .and_hms_opt(9, 30, 0).unwrap()
            .and_utc();
        let delta = provider.compute_delta(100.0, entry_time).await.unwrap();

        // Should be positive (call) and less than 1
        assert!(delta > 0.0 && delta < 1.0, "Delta should be per-share, got {}", delta);
    }

    #[tokio::test]
    async fn test_per_share_convention() {
        let expiration = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let leg = OptionLeg::new(
            "SPY".to_string(),
            Strike::new(Decimal::from(100)).unwrap(),
            expiration,
            OptionType::Call,
        );
        let position = LegPosition::Long;

        let trade = TestTrade {
            legs: vec![(leg, position)],
        };

        let mut provider = EntryVolatilityProvider::new_entry_iv(trade, 0.25, 0.05);

        let entry_time = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            .and_hms_opt(9, 30, 0).unwrap()
            .and_utc();
        let delta = provider.compute_delta(100.0, entry_time).await.unwrap();

        // Delta should be per-share (< 2.0), NOT multiplied by 100
        assert!(delta.abs() < 2.0, "Delta should be per-share, got {}", delta);
    }

    #[tokio::test]
    async fn test_expired_option() {
        let expiration = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let leg = OptionLeg::new(
            "SPY".to_string(),
            Strike::new(Decimal::from(100)).unwrap(),
            expiration,
            OptionType::Call,
        );
        let position = LegPosition::Long;

        let trade = TestTrade {
            legs: vec![(leg, position)],
        };

        let mut provider = EntryVolatilityProvider::new_entry_hv(trade, 0.20, 0.05);

        // Check delta after expiration
        let after_exp = NaiveDate::from_ymd_opt(2024, 2, 1).unwrap()
            .and_hms_opt(9, 30, 0).unwrap()
            .and_utc();
        let delta = provider.compute_delta(100.0, after_exp).await.unwrap();

        // Expired option should have 0 delta
        assert_eq!(delta, 0.0);
    }

    #[tokio::test]
    async fn test_short_position_negative_delta() {
        let expiration = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let leg = OptionLeg::new(
            "SPY".to_string(),
            Strike::new(Decimal::from(100)).unwrap(),
            expiration,
            OptionType::Call,
        );
        let position = LegPosition::Short;  // Short position

        let trade = TestTrade {
            legs: vec![(leg, position)],
        };

        let mut provider = EntryVolatilityProvider::new_entry_hv(trade, 0.20, 0.05);

        let entry_time = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()
            .and_hms_opt(9, 30, 0).unwrap()
            .and_utc();
        let delta = provider.compute_delta(100.0, entry_time).await.unwrap();

        // Short call should have negative delta
        assert!(delta < 0.0, "Short call delta should be negative, got {}", delta);
    }
}
