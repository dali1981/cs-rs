//! Historical average IV delta provider
//!
//! Computes delta using averaged implied volatility over a lookback period.
//! Smooths out IV noise by averaging recent market IV values.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use std::collections::HashMap;
use cs_domain::hedging::DeltaProvider;
use cs_domain::repositories::{EquityDataRepository, OptionsDataRepository};
use cs_domain::trade::CompositeTrade;
use cs_domain::TradingCalendar;
use finq_core::OptionType;
use super::common::compute_position_delta_with_vol_lookup;
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

        // Pre-compute average IVs for all legs in the trade
        let mut leg_ivs: HashMap<(Decimal, NaiveDate, bool), f64> = HashMap::new();

        for (leg, _) in self.trade.legs() {
            let key = (leg.strike.value(), leg.expiration, leg.option_type == OptionType::Call);

            // Only compute if not already cached
            if !leg_ivs.contains_key(&key) {
                let avg_iv = self.compute_average_iv_for_leg(
                    leg.strike.value(),
                    leg.expiration,
                    leg.option_type == OptionType::Call,
                    current_date,
                )
                .await
                .unwrap_or(0.30); // Fallback to 30% if computation fails

                leg_ivs.insert(key, avg_iv);
            }
        }

        // Compute position delta using pre-computed averaged IVs via shared helper
        let position_delta = compute_position_delta_with_vol_lookup(
            &self.trade,
            spot,
            timestamp,
            |leg, _spot| {
                let key = (leg.strike.value(), leg.expiration, leg.option_type == OptionType::Call);
                *leg_ivs.get(&key).unwrap_or(&0.30)
            },
            self.risk_free_rate,
        );

        Ok(position_delta)
    }

    fn name(&self) -> &'static str {
        "historical_average_iv"
    }
}
