use chrono::{DateTime, Utc};
use std::sync::Arc;

use cs_domain::{
    CompositeTrade, EquityDataRepository, OptionsDataRepository,
    PositionSnapshot, PositionGreeks,
};
use cs_analytics::bs_greeks;
use finq_core::OptionType;
use rust_decimal::prelude::ToPrimitive;

/// Collects position snapshots during hedging period
///
/// Captures daily state (spot, IV, Greeks, hedge position) for P&L attribution.
pub struct SnapshotCollector {
    equity_repo: Arc<dyn EquityDataRepository>,
    options_repo: Arc<dyn OptionsDataRepository>,
}

impl SnapshotCollector {
    pub fn new(
        equity_repo: Arc<dyn EquityDataRepository>,
        options_repo: Arc<dyn OptionsDataRepository>,
    ) -> Self {
        Self {
            equity_repo,
            options_repo,
        }
    }

    /// Capture position snapshot at a specific time
    ///
    /// Recomputes Greeks from current spot, IV, and time-to-expiry.
    ///
    /// # Arguments
    /// * `trade` - The composite trade to snapshot
    /// * `at_time` - Snapshot timestamp
    /// * `volatility` - Volatility to use for Greeks computation (IV or HV)
    /// * `hedge_shares` - Current hedge position (negative = short)
    /// * `contract_multiplier` - Contract multiplier (typically 100)
    ///
    /// # Returns
    /// PositionSnapshot with freshly computed Greeks
    pub async fn capture_snapshot<T>(
        &self,
        trade: &T,
        at_time: DateTime<Utc>,
        volatility: f64,
        hedge_shares: i32,
        contract_multiplier: i32,
    ) -> Result<PositionSnapshot, String>
    where
        T: CompositeTrade,
    {
        let symbol = trade.symbol();

        // Get spot price at snapshot time
        let spot = self
            .equity_repo
            .get_spot_price(symbol, at_time)
            .await
            .map_err(|e| format!("Failed to get spot price: {}", e))?;

        let spot_f64 = spot.to_f64();

        // Compute position Greeks
        let position_greeks = self.compute_position_greeks(
            trade,
            spot_f64,
            volatility,
            at_time,
            contract_multiplier,
        )?;

        // Create snapshot
        Ok(PositionSnapshot::new(
            at_time,
            spot_f64,
            volatility,
            position_greeks,
            hedge_shares,
        ))
    }

    /// Compute position-level Greeks from trade legs
    ///
    /// Iterates over all legs, computes per-share Greeks for each,
    /// and aggregates into position-level Greeks.
    fn compute_position_greeks<T>(
        &self,
        trade: &T,
        spot: f64,
        volatility: f64,
        at_time: DateTime<Utc>,
        contract_multiplier: i32,
    ) -> Result<PositionGreeks, String>
    where
        T: CompositeTrade,
    {
        let risk_free_rate = 0.05;

        let mut total_delta = 0.0;
        let mut total_gamma = 0.0;
        let mut total_theta = 0.0;
        let mut total_vega = 0.0;

        for (leg, position) in trade.legs() {
            let tte = (leg.expiration - at_time.date_naive()).num_days() as f64 / 365.0;

            // Skip expired options
            if tte <= 0.0 {
                continue;
            }

            let strike = leg
                .strike
                .value()
                .to_f64()
                .ok_or("Failed to convert strike")?;
            let is_call = leg.option_type == OptionType::Call;

            // Compute all Greeks at once
            let greeks = bs_greeks(spot, strike, tte, volatility, is_call, risk_free_rate);

            // Aggregate with position sign and multiplier
            let sign = position.sign();
            let multiplier = contract_multiplier as f64;

            total_delta += greeks.delta * sign * multiplier;
            total_gamma += greeks.gamma * sign * multiplier;
            total_theta += greeks.theta * sign * multiplier;
            total_vega += greeks.vega * sign * multiplier;
        }

        Ok(PositionGreeks {
            delta: total_delta,
            gamma: total_gamma,
            theta: total_theta,
            vega: total_vega,
        })
    }
}
