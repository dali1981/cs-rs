//! Current historical volatility delta provider
//!
//! Recomputes HV at each rehedge from recent underlying price history,
//! then uses that HV to compute delta via Black-Scholes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use cs_analytics::{bs_delta, realized_volatility};
use cs_domain::hedging::DeltaProvider;
use cs_domain::repositories::EquityDataRepository;
use cs_domain::trade::CompositeTrade;
use finq_core::OptionType;

/// Recompute HV at each rehedge from recent underlying prices
///
/// This provider tracks actual underlying volatility evolution by
/// recomputing HV from the price history at each rehedge point.
///
/// # Delta Convention
/// Returns per-share delta (e.g., 0.5 for ATM call, NOT 50)
pub struct CurrentHVProvider<T: CompositeTrade> {
    trade: T,
    equity_repo: Arc<dyn EquityDataRepository>,
    symbol: String,
    window: u32,
    risk_free_rate: f64,
}

impl<T: CompositeTrade> CurrentHVProvider<T> {
    /// Create new provider
    ///
    /// # Arguments
    /// * `trade` - The composite trade to compute delta for
    /// * `equity_repo` - Repository for fetching equity price data
    /// * `symbol` - Underlying symbol
    /// * `window` - Lookback window in days for HV computation
    /// * `risk_free_rate` - Risk-free rate for Black-Scholes
    pub fn new(
        trade: T,
        equity_repo: Arc<dyn EquityDataRepository>,
        symbol: String,
        window: u32,
        risk_free_rate: f64,
    ) -> Self {
        Self {
            trade,
            equity_repo,
            symbol,
            window,
            risk_free_rate,
        }
    }

    /// Compute HV at a specific time using recent price history
    async fn compute_hv(&self, at_time: DateTime<Utc>) -> Result<f64, String> {
        let end_date = at_time.date_naive();

        let bars = self.equity_repo
            .get_bars(&self.symbol, end_date)
            .await
            .map_err(|e| format!("Failed to get bars: {}", e))?;

        let closes: Vec<f64> = bars.column("close")
            .map_err(|_| "No close column".to_string())?
            .f64()
            .map_err(|_| "Invalid close type".to_string())?
            .into_no_null_iter()
            .collect();

        realized_volatility(&closes, self.window as usize, 252.0)
            .ok_or_else(|| "Insufficient data for HV computation".to_string())
    }
}

#[async_trait]
impl<T: CompositeTrade + Send + Sync> DeltaProvider for CurrentHVProvider<T> {
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
        // 1. Compute current HV from recent price history
        let current_hv = self.compute_hv(timestamp).await?;

        // 2. Compute delta using current HV (per-share, NO multiplier)
        let position_delta: f64 = self.trade.legs().iter().map(|(leg, position)| {
            let tte = (leg.expiration - timestamp.date_naive()).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                return 0.0;  // Expired
            }

            let is_call = leg.option_type == OptionType::Call;
            let strike = leg.strike.value().to_f64().unwrap_or(0.0);

            // Per-share delta from Black-Scholes
            let leg_delta = bs_delta(
                spot,
                strike,
                tte,
                current_hv,
                is_call,
                self.risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1)
            // NO multiplier here - we return per-share delta
            leg_delta * position.sign()
        }).sum();

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        "current_hv"
    }
}
