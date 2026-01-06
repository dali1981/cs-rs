//! Greeks computation for P&L attribution
//!
//! Provides utilities to recompute position-level Greeks from IV surfaces
//! or flat volatility values for attribution snapshots.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;

use cs_domain::{CompositeTrade, PositionGreeks};
use cs_analytics::{bs_greeks, Greeks, IVSurface, PricingIVProvider};
use finq_core::OptionType;

/// Computes position-level Greeks for attribution
///
/// Provides two computation modes:
/// 1. Flat volatility - uses single vol for all legs (fast)
/// 2. IV surface - uses per-leg IV from surface (accurate)
pub struct GreeksComputer<'a, T: CompositeTrade> {
    trade: &'a T,
    contract_multiplier: i32,
    risk_free_rate: f64,
}

impl<'a, T: CompositeTrade> GreeksComputer<'a, T> {
    pub fn new(trade: &'a T, contract_multiplier: i32, risk_free_rate: f64) -> Self {
        Self {
            trade,
            contract_multiplier,
            risk_free_rate,
        }
    }

    /// Compute position Greeks from a single volatility value
    ///
    /// Used for EntryHV, EntryIV, CurrentHV modes where we have one vol for all legs.
    ///
    /// # Arguments
    /// * `spot` - Current spot price
    /// * `volatility` - Volatility to use for all legs
    /// * `at_time` - Current time (for DTE calculation)
    ///
    /// # Returns
    /// Position-level Greeks (scaled by multiplier)
    pub fn compute_with_flat_vol(
        &self,
        spot: f64,
        volatility: f64,
        at_time: DateTime<Utc>,
    ) -> PositionGreeks {
        let mut total = Greeks::default();

        for (leg, position) in self.trade.legs() {
            let tte = (leg.expiration - at_time.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                continue;
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            let leg_greeks = bs_greeks(
                spot,
                strike,
                tte,
                volatility,
                is_call,
                self.risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1)
            let sign = position.sign();
            total.delta += leg_greeks.delta * sign;
            total.gamma += leg_greeks.gamma * sign;
            total.theta += leg_greeks.theta * sign;
            total.vega += leg_greeks.vega * sign;
        }

        // Scale to position level
        PositionGreeks::from_per_share(&total, self.contract_multiplier)
    }

    /// Compute position Greeks from IV surface (per-leg IV)
    ///
    /// Used for CurrentMarketIV mode where each leg can have different IV.
    ///
    /// # Arguments
    /// * `spot` - Current spot price
    /// * `surface` - IV surface with per-strike IVs
    /// * `provider` - IV interpolation provider
    /// * `at_time` - Current time (for DTE calculation)
    ///
    /// # Returns
    /// Position-level Greeks (scaled by multiplier)
    pub fn compute_with_surface(
        &self,
        spot: f64,
        surface: &IVSurface,
        provider: &dyn PricingIVProvider,
        at_time: DateTime<Utc>,
    ) -> PositionGreeks {
        let mut total = Greeks::default();

        for (leg, position) in self.trade.legs() {
            let tte = (leg.expiration - at_time.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                continue;
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value();
            let strike_f64 = strike.to_f64().unwrap_or(0.0);

            // Get IV from surface for this specific leg
            let iv = provider
                .get_iv(surface, strike, leg.expiration, is_call)
                .unwrap_or(0.30); // Fallback

            let leg_greeks = bs_greeks(
                spot,
                strike_f64,
                tte,
                iv,
                is_call,
                self.risk_free_rate,
            );

            let sign = position.sign();
            total.delta += leg_greeks.delta * sign;
            total.gamma += leg_greeks.gamma * sign;
            total.theta += leg_greeks.theta * sign;
            total.vega += leg_greeks.vega * sign;
        }

        PositionGreeks::from_per_share(&total, self.contract_multiplier)
    }

    /// Compute average IV across position legs from surface
    ///
    /// Used for vega attribution when we need a single IV value.
    ///
    /// # Arguments
    /// * `surface` - IV surface
    /// * `provider` - IV interpolation provider
    /// * `timestamp` - Current time
    ///
    /// # Returns
    /// Average IV across all legs, or 0.30 if no valid IVs found
    pub fn compute_position_avg_iv(
        &self,
        surface: &IVSurface,
        provider: &dyn PricingIVProvider,
        _timestamp: DateTime<Utc>,
    ) -> f64 {
        let ivs: Vec<f64> = self
            .trade
            .legs()
            .iter()
            .filter_map(|(leg, _)| {
                let is_call = leg.option_type == OptionType::Call;
                provider.get_iv(surface, leg.strike.value(), leg.expiration, is_call)
            })
            .collect();

        if ivs.is_empty() {
            0.30 // Fallback
        } else {
            ivs.iter().sum::<f64>() / ivs.len() as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cs_analytics::Greeks;
    use cs_domain::{OptionLeg, Position, PositionType, Strike};
    use finq_core::OptionType;
    use rust_decimal::Decimal;

    // Mock trade for testing
    struct MockTrade {
        legs: Vec<(OptionLeg, PositionType)>,
    }

    impl CompositeTrade for MockTrade {
        fn legs(&self) -> &[(OptionLeg, PositionType)] {
            &self.legs
        }

        fn symbol(&self) -> &str {
            "SPY"
        }
    }

    #[test]
    fn test_compute_with_flat_vol_single_leg() {
        // Single ATM call
        let expiration = Utc::now().date_naive() + chrono::Duration::days(30);
        let leg = OptionLeg {
            symbol: "SPY".to_string(),
            expiration,
            strike: Strike::new(Decimal::new(500, 0)),
            option_type: OptionType::Call,
        };

        let trade = MockTrade {
            legs: vec![(leg, PositionType::Long)],
        };

        let computer = GreeksComputer::new(&trade, 100, 0.05);
        let greeks = computer.compute_with_flat_vol(500.0, 0.25, Utc::now());

        // For ATM call with 30 DTE, delta should be ~0.5 × 100 = 50
        assert!(greeks.delta > 40.0 && greeks.delta < 60.0);
        // Gamma should be positive
        assert!(greeks.gamma > 0.0);
        // Theta should be negative (time decay)
        assert!(greeks.theta < 0.0);
        // Vega should be positive
        assert!(greeks.vega > 0.0);
    }

    #[test]
    fn test_compute_with_flat_vol_straddle() {
        // ATM straddle
        let expiration = Utc::now().date_naive() + chrono::Duration::days(30);
        let call = OptionLeg {
            symbol: "SPY".to_string(),
            expiration,
            strike: Strike::new(Decimal::new(500, 0)),
            option_type: OptionType::Call,
        };
        let put = OptionLeg {
            symbol: "SPY".to_string(),
            expiration,
            strike: Strike::new(Decimal::new(500, 0)),
            option_type: OptionType::Put,
        };

        let trade = MockTrade {
            legs: vec![
                (call, PositionType::Long),
                (put, PositionType::Long),
            ],
        };

        let computer = GreeksComputer::new(&trade, 100, 0.05);
        let greeks = computer.compute_with_flat_vol(500.0, 0.25, Utc::now());

        // For ATM straddle, delta should be near zero
        assert!(greeks.delta.abs() < 5.0);
        // Gamma should be positive (long gamma)
        assert!(greeks.gamma > 0.0);
        // Theta should be negative (time decay)
        assert!(greeks.theta < 0.0);
        // Vega should be positive (long vega)
        assert!(greeks.vega > 0.0);
    }

    #[test]
    fn test_compute_with_flat_vol_short_position() {
        // Short ATM call
        let expiration = Utc::now().date_naive() + chrono::Duration::days(30);
        let leg = OptionLeg {
            symbol: "SPY".to_string(),
            expiration,
            strike: Strike::new(Decimal::new(500, 0)),
            option_type: OptionType::Call,
        };

        let trade = MockTrade {
            legs: vec![(leg, PositionType::Short)],
        };

        let computer = GreeksComputer::new(&trade, 100, 0.05);
        let greeks = computer.compute_with_flat_vol(500.0, 0.25, Utc::now());

        // Short call should have negative delta
        assert!(greeks.delta < -40.0 && greeks.delta > -60.0);
        // Short gamma
        assert!(greeks.gamma < 0.0);
        // Positive theta (benefit from time decay)
        assert!(greeks.theta > 0.0);
        // Negative vega
        assert!(greeks.vega < 0.0);
    }
}
