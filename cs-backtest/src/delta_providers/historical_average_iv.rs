//! Historical average IV delta provider
//!
//! Computes delta using averaged implied volatility over a lookback period.
//! Smooths out IV noise by averaging recent market IV values.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use cs_analytics::bs_delta;
use cs_domain::hedging::DeltaProvider;
use cs_domain::repositories::{EquityDataRepository, OptionsDataRepository};
use cs_domain::trade::CompositeTrade;
use cs_domain::TradingCalendar;
use finq_core::OptionType;
use crate::iv_surface_builder::build_iv_surface;

/// Use historical average IV over lookback period
///
/// For each leg, this provider samples IVs from the lookback window,
/// averages them, and uses that to compute delta. This smooths out
/// IV noise compared to using spot IV.
///
/// # Delta Convention
/// Returns per-share delta (e.g., 0.5 for ATM call, NOT 50)
pub struct HistoricalAverageIVProvider<T: CompositeTrade> {
    trade: T,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    symbol: String,
    lookback_days: u32,
    risk_free_rate: f64,
}

impl<T: CompositeTrade> HistoricalAverageIVProvider<T> {
    /// Create new provider
    ///
    /// # Arguments
    /// * `trade` - The composite trade to compute delta for
    /// * `options_repo` - Repository for fetching option chain data
    /// * `equity_repo` - Repository for fetching equity price data
    /// * `symbol` - Underlying symbol
    /// * `lookback_days` - Number of days to look back for IV averaging
    /// * `risk_free_rate` - Risk-free rate for Black-Scholes
    pub fn new(
        trade: T,
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        symbol: String,
        lookback_days: u32,
        risk_free_rate: f64,
    ) -> Self {
        Self {
            trade,
            options_repo,
            equity_repo,
            symbol,
            lookback_days,
            risk_free_rate,
        }
    }

    /// Compute historical average IV for a specific leg over lookback window
    async fn compute_average_iv_for_leg(
        &self,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
        current_date: NaiveDate,
    ) -> Result<f64, String> {
        let end_date = current_date;
        let start_date = current_date - chrono::Duration::days(self.lookback_days as i64);

        let trading_days: Vec<NaiveDate> = TradingCalendar::trading_days_between(start_date, end_date)
            .collect();

        if trading_days.is_empty() {
            return Err("No trading days in lookback window".to_string());
        }

        let mut ivs = Vec::new();

        // Sample IVs from historical days (limit to avoid performance issues)
        let sample_size = trading_days.len().min(10); // Sample at most 10 days
        let step = if trading_days.len() > sample_size {
            trading_days.len() / sample_size
        } else {
            1
        };

        for (idx, date) in trading_days.iter().enumerate() {
            if idx % step != 0 {
                continue; // Skip non-sampled days
            }

            // Get option chain for this historical day
            let chain_df = match self.options_repo.get_option_bars(&self.symbol, *date).await {
                Ok(df) => df,
                Err(_) => continue, // Skip days with missing data
            };

            // Get spot price for this day (use close price from bars)
            let bars_df = match self.equity_repo.get_bars(&self.symbol, *date).await {
                Ok(df) => df,
                Err(_) => continue,
            };

            // Extract close price as spot (last bar's close)
            let spot = if let Ok(close_series) = bars_df.column("close") {
                if let Ok(close_f64) = close_series.f64() {
                    // Get last non-null close
                    close_f64.into_iter()
                        .filter_map(|x| x)
                        .last()
                        .unwrap_or(100.0)
                } else {
                    continue;
                }
            } else {
                continue;
            };

            // Build IV surface
            let pricing_time = cs_domain::eastern_to_utc(
                *date,
                cs_domain::MarketTime::DEFAULT_ENTRY.to_naive_time(),
            );
            let iv_surface = match build_iv_surface(&chain_df, spot, pricing_time, &self.symbol) {
                Some(surface) => surface,
                None => continue,
            };

            // Get IV for this specific leg
            if let Some(iv) = iv_surface.get_iv(strike, expiration, is_call) {
                ivs.push(iv);
            }
        }

        if ivs.is_empty() {
            return Err("Could not compute historical IV - no valid data points".to_string());
        }

        // Return average IV
        Ok(ivs.iter().sum::<f64>() / ivs.len() as f64)
    }
}

#[async_trait]
impl<T: CompositeTrade + Send + Sync> DeltaProvider for HistoricalAverageIVProvider<T> {
    async fn compute_delta(&mut self, spot: f64, timestamp: DateTime<Utc>) -> Result<f64, String> {
        let current_date = timestamp.date_naive();

        // Compute position delta using averaged IVs (per-share, NO multiplier)
        let mut position_delta = 0.0;

        for (leg, leg_position) in self.trade.legs() {
            let tte = (leg.expiration - current_date).num_days() as f64 / 365.0;
            if tte <= 0.0 {
                continue;  // Skip expired legs
            }

            // Compute average IV for this leg over lookback window
            let avg_iv = self.compute_average_iv_for_leg(
                leg.strike.value(),
                leg.expiration,
                leg.option_type == OptionType::Call,
                current_date,
            )
            .await
            .unwrap_or(0.30);  // Fallback to 30% if computation fails

            let strike_f64 = leg.strike.value().to_f64().unwrap_or(0.0);
            let is_call = leg.option_type == OptionType::Call;

            // Per-share delta from Black-Scholes
            let leg_delta = bs_delta(
                spot,
                strike_f64,
                tte,
                avg_iv,
                is_call,
                self.risk_free_rate,
            );

            // Apply position sign (long = +1, short = -1)
            // NO multiplier here - we return per-share delta
            position_delta += leg_delta * leg_position.sign();
        }

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        "historical_average_iv"
    }
}
