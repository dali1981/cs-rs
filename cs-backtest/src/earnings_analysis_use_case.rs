// Earnings Analysis Use Case
//
// Analyzes expected vs actual moves on earnings events

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::sync::Arc;
use thiserror::Error;

use cs_analytics::{AtmMethod, StraddlePriceComputer};
use cs_domain::{
    entities::EarningsEvent,
    repositories::{EarningsRepository, EquityDataRepository, OptionsDataRepository, RepositoryError},
    timing::EarningsTradeTiming,
    value_objects::{AtmIvConfig, EarningsOutcome, EarningsSummaryStats, TimingConfig},
};

/// Result of earnings analysis
#[derive(Debug, serde::Serialize)]
pub struct EarningsAnalysisResult {
    pub symbol: String,
    pub outcomes: Vec<EarningsOutcome>,
    pub summary: EarningsSummaryStats,
}

/// Errors during earnings analysis
#[derive(Debug, Error)]
pub enum EarningsAnalysisError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("No earnings events found for {symbol} in date range")]
    NoEarningsEvents { symbol: String },
    #[error("No spot price for {symbol} at {time}")]
    NoSpotPrice { symbol: String, time: String },
    #[error("No straddle data for {symbol} on {date}")]
    NoStraddleData { symbol: String, date: NaiveDate },
}

/// Use case for analyzing earnings expected vs actual moves
pub struct EarningsAnalysisUseCase<E, O, R>
where
    E: EquityDataRepository,
    O: OptionsDataRepository,
    R: EarningsRepository,
{
    equity_repo: Arc<E>,
    options_repo: Arc<O>,
    earnings_repo: Arc<R>,
    timing_service: EarningsTradeTiming,
}

impl<E, O, R> EarningsAnalysisUseCase<E, O, R>
where
    E: EquityDataRepository,
    O: OptionsDataRepository,
    R: EarningsRepository,
{
    pub fn new(
        equity_repo: Arc<E>,
        options_repo: Arc<O>,
        earnings_repo: Arc<R>,
        timing_config: TimingConfig,
    ) -> Self {
        Self {
            equity_repo,
            options_repo,
            earnings_repo,
            timing_service: EarningsTradeTiming::new(timing_config),
        }
    }

    pub fn with_default_timing(
        equity_repo: Arc<E>,
        options_repo: Arc<O>,
        earnings_repo: Arc<R>,
    ) -> Self {
        Self::new(equity_repo, options_repo, earnings_repo, TimingConfig::default())
    }

    /// Analyze all earnings events for a symbol over a date range
    ///
    /// For each earnings event:
    /// 1. Get pre-earnings straddle price (close before announcement)
    /// 2. Compute expected move
    /// 3. Get post-earnings spot price
    /// 4. Compare actual vs expected
    /// 5. Determine if gamma dominated vega
    pub async fn analyze_earnings(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        config: &AtmIvConfig,
    ) -> Result<EarningsAnalysisResult, EarningsAnalysisError> {
        // Load earnings events
        let events = self
            .earnings_repo
            .load_earnings(start_date, end_date, Some(&[symbol.to_string()]))
            .await?;

        if events.is_empty() {
            return Err(EarningsAnalysisError::NoEarningsEvents {
                symbol: symbol.to_string(),
            });
        }

        println!("Found {} earnings events for {}", events.len(), symbol);

        let mut outcomes = Vec::new();

        for event in &events {
            print!("Analyzing {} earnings on {}... ", event.symbol, event.earnings_date);

            match self.analyze_single_event(event, config).await {
                Ok(outcome) => {
                    println!("✓ Expected: {:.2}%, Actual: {:.2}%, Ratio: {:.2}x",
                             outcome.expected_move_pct,
                             outcome.actual_move_pct,
                             outcome.move_ratio);
                    outcomes.push(outcome);
                }
                Err(e) => {
                    println!("✗ {}", e);
                }
            }
        }

        if outcomes.is_empty() {
            return Err(EarningsAnalysisError::NoEarningsEvents {
                symbol: symbol.to_string(),
            });
        }

        // Compute summary statistics
        let summary = EarningsSummaryStats::from_outcomes(&outcomes);

        Ok(EarningsAnalysisResult {
            symbol: symbol.to_string(),
            outcomes,
            summary,
        })
    }

    /// Analyze a single earnings event
    async fn analyze_single_event(
        &self,
        event: &EarningsEvent,
        config: &AtmIvConfig,
    ) -> Result<EarningsOutcome, EarningsAnalysisError> {
        // Get pre-earnings state (entry time)
        let entry_time = self.timing_service.entry_datetime(event);
        let pre_spot = self
            .equity_repo
            .get_spot_price(&event.symbol, entry_time)
            .await
            .map_err(|_| EarningsAnalysisError::NoSpotPrice {
                symbol: event.symbol.clone(),
                time: entry_time.to_string(),
            })?;

        // Get options at entry time and compute straddle
        let entry_date = self.timing_service.entry_date(event);
        let options_df = self
            .options_repo
            .get_option_bars(&event.symbol, entry_date)
            .await
            .map_err(|_| EarningsAnalysisError::NoStraddleData {
                symbol: event.symbol.clone(),
                date: entry_date,
            })?;

        // Extract option data
        let option_data = self.extract_option_data(&options_df)?;
        let pre_spot_f64 = pre_spot.to_f64();

        // Compute straddle
        let atm_method = match config.atm_strike_method {
            cs_domain::value_objects::AtmMethod::Closest => AtmMethod::Closest,
            cs_domain::value_objects::AtmMethod::BelowSpot => AtmMethod::BelowSpot,
            cs_domain::value_objects::AtmMethod::AboveSpot => AtmMethod::AboveSpot,
        };

        let straddle = StraddlePriceComputer::compute_straddle(
            &option_data,
            pre_spot_f64,
            entry_date,
            None, // Use nearest expiration
            1,    // Min DTE
            atm_method,
        )
        .ok_or_else(|| EarningsAnalysisError::NoStraddleData {
            symbol: event.symbol.clone(),
            date: entry_date,
        })?;

        let pre_straddle = Decimal::try_from(straddle.straddle_price).unwrap_or(Decimal::ZERO);

        // Compute pre-earnings IV (simplified: use 30d if available)
        // In a full implementation, we'd compute ATM IV at entry time
        let pre_iv_30d = 0.40; // Placeholder - should compute from options

        // Get post-earnings state (exit time)
        let exit_time = self.timing_service.exit_datetime(event);
        let post_spot = self
            .equity_repo
            .get_spot_price(&event.symbol, exit_time)
            .await
            .map_err(|_| EarningsAnalysisError::NoSpotPrice {
                symbol: event.symbol.clone(),
                time: exit_time.to_string(),
            })?;

        // Compute post-earnings IV (optional, for IV crush)
        let post_iv_30d = None; // Placeholder - could compute from exit options

        // Create outcome
        let outcome = EarningsOutcome::new(
            event.symbol.clone(),
            event.earnings_date,
            event.earnings_time,
            pre_spot.value,
            pre_straddle,
            pre_iv_30d,
            post_spot.value,
            post_iv_30d,
        );

        Ok(outcome)
    }

    /// Extract option data from DataFrame
    fn extract_option_data(
        &self,
        df: &polars::frame::DataFrame,
    ) -> Result<Vec<(f64, NaiveDate, f64, bool)>, EarningsAnalysisError> {
        use polars::prelude::*;

        let strikes = df
            .column("strike")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .f64()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let expirations = df
            .column("expiration")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .date()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let closes = df
            .column("close")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .f64()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let option_types = df
            .column("option_type")
            .map_err(|e| RepositoryError::Polars(e.to_string()))?
            .str()
            .map_err(|e| RepositoryError::Polars(e.to_string()))?;

        let mut options = Vec::new();

        for i in 0..df.height() {
            let (strike, exp_days, close, opt_type) = match (
                strikes.get(i),
                expirations.get(i),
                closes.get(i),
                option_types.get(i),
            ) {
                (Some(s), Some(e), Some(c), Some(t)) => (s, e, c, t),
                _ => continue,
            };

            if close <= 0.0 || strike <= 0.0 {
                continue;
            }

            // Convert polars date (days since epoch) to NaiveDate
            let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
            let expiration = epoch + chrono::Duration::days(exp_days as i64);

            let is_call = opt_type == "call";

            options.push((strike, expiration, close, is_call));
        }

        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests would go here - require mock repositories
}
