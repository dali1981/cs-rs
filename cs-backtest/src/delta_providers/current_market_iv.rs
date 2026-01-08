//! Current market IV delta provider
//!
//! Builds a fresh IV surface at each rehedge from the option chain,
//! providing the most accurate delta computation at the cost of performance.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use cs_domain::hedging::DeltaProvider;
use cs_domain::repositories::{EquityDataRepository, OptionsDataRepository};
use cs_domain::trade::CompositeTrade;
use super::common::compute_position_delta_with_vol_lookup;
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
        // Build IV surface at current time
        let chain_df = self.options_repo
            .get_option_bars(&self.symbol, timestamp.date_naive())
            .await
            .map_err(|e| format!("Failed to get option chain at {}: {}", timestamp, e))?;

        let iv_surface = build_iv_surface(&chain_df, spot, timestamp, &self.symbol)
            .ok_or_else(|| format!("Failed to build IV surface at {}", timestamp))?;

        // Compute position delta using current market IVs via shared helper
        let position_delta = compute_position_delta_with_vol_lookup(
            &self.trade,
            spot,
            timestamp,
            |leg, _spot| {
                // Get IV from surface for this specific leg
                iv_surface
                    .get_iv(leg.strike.value(), leg.expiration, leg.option_type == finq_core::OptionType::Call)
                    .unwrap_or(0.30) // Fallback to 30% if not found
            },
            self.risk_free_rate,
        );

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        "current_market_iv"
    }
}
