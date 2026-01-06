//! Current market IV delta provider
//!
//! Builds a fresh IV surface at each rehedge from the option chain,
//! providing the most accurate delta computation at the cost of performance.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use cs_analytics::bs_delta;
use cs_domain::hedging::DeltaProvider;
use cs_domain::repositories::{EquityDataRepository, OptionsDataRepository};
use cs_domain::trade::CompositeTrade;
use finq_core::OptionType;
use crate::iv_surface_builder::build_iv_surface;

/// Build fresh IV surface at each rehedge from current market data
///
/// This is the most accurate method as it uses real-time market IVs,
/// but it's expensive as it requires market data lookups and IV surface construction.
///
/// # Delta Convention
/// Returns per-share delta (e.g., 0.5 for ATM call, NOT 50)
pub struct CurrentMarketIVProvider<T: CompositeTrade> {
    trade: T,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    symbol: String,
    risk_free_rate: f64,
}

impl<T: CompositeTrade> CurrentMarketIVProvider<T> {
    /// Create new provider
    ///
    /// # Arguments
    /// * `trade` - The composite trade to compute delta for
    /// * `options_repo` - Repository for fetching option chain data
    /// * `equity_repo` - Repository for fetching equity price data
    /// * `symbol` - Underlying symbol
    /// * `risk_free_rate` - Risk-free rate for Black-Scholes
    pub fn new(
        trade: T,
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        symbol: String,
        risk_free_rate: f64,
    ) -> Self {
        Self {
            trade,
            options_repo,
            equity_repo,
            symbol,
            risk_free_rate,
        }
    }
}

#[async_trait]
impl<T: CompositeTrade + Send + Sync> DeltaProvider for CurrentMarketIVProvider<T> {
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
        // 1. Build IV surface at current time
        let chain_df = self.options_repo
            .get_option_bars(&self.symbol, timestamp.date_naive())
            .await
            .map_err(|e| format!("Failed to get option chain at {}: {}", timestamp, e))?;

        let iv_surface = build_iv_surface(&chain_df, spot, timestamp, &self.symbol)
            .ok_or_else(|| format!("Failed to build IV surface at {}", timestamp))?;

        // 2. Compute position delta using current market IVs (per-share, NO multiplier)
        let position_delta: f64 = self.trade.legs().iter().map(|(leg, leg_position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                return 0.0;  // Expired
            }

            let strike_f64 = leg.strike.value().to_f64().unwrap_or(0.0);
            let is_call = leg.option_type == OptionType::Call;

            // Get IV from surface for this specific leg
            let leg_iv = iv_surface
                .get_iv(leg.strike.value(), leg.expiration, is_call)
                .unwrap_or(0.30);  // Fallback to 30% if not found

            // Per-share delta from Black-Scholes
            let leg_delta = bs_delta(
                spot,
                strike_f64,
                tte,
                leg_iv,
                is_call,
                self.risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1)
            // NO multiplier here - we return per-share delta
            leg_delta * leg_position.sign()
        }).sum();

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        "current_market_iv"
    }
}
