//! Hedging executor for BacktestUseCase strategies
//!
//! This module provides hedging support for the TradeStrategy trait,
//! applying delta hedging at the backtest execution level.
//!
//! # Architecture
//!
//! Hedging is path-dependent - it depends on spot price movements between
//! entry and exit, not just the final values. This module:
//!
//! 1. Takes the trade result (with Greeks from entry)
//! 2. Iterates through rehedge times
//! 3. Fetches spot prices at each time point
//! 4. Computes delta using the configured method (gamma approx, BS reprice, etc.)
//! 5. Applies hedging decisions based on delta threshold
//!
//! # Usage Modes
//!
//! ## 1. Result-only hedging (GammaApproximation) - BacktestUseCase level
//!
//! ```ignore
//! // In BacktestUseCase::execute_tradable_batch, after strategy returns:
//! if let Some(ref hedge_config) = exec_config.hedge_config {
//!     apply_hedging_from_result(
//!         &mut result,
//!         equity_repo,
//!         hedge_config,
//!         &timing,
//!         entry_time,
//!         exit_time,
//!     ).await;
//! }
//! ```
//!
//! ## 2. Full trade hedging (EntryIV, EntryHV, etc.) - Strategy level
//!
//! ```ignore
//! // In strategy's execute_trade, after simulation:
//! if let Some(ref hedge_config) = exec_config.hedge_config {
//!     apply_hedging(
//!         &trade,
//!         &mut result,
//!         equity_repo,
//!         options_repo,
//!         hedge_config,
//!         &timing,
//!         entry_time,
//!         exit_time,
//!     ).await;
//! }
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    HedgeConfig, HedgePosition, HedgeStrategy, DeltaComputation,
    GenericHedgeState, TradeResult, CompositeTrade,
};

use crate::timing_strategy::TimingStrategy;
use crate::delta_providers::*;

/// Apply delta hedging to a trade
///
/// This function applies hedging using the full trade structure, enabling
/// proper delta recomputation at each rehedge point.
///
/// # Type Parameters
///
/// * `T` - Trade type implementing CompositeTrade (has legs with strikes, expirations)
/// * `R` - Result type implementing TradeResult
///
/// # Arguments
///
/// * `trade` - The trade object with full option structure
/// * `result` - Mutable reference to the trade result
/// * `equity_repo` - Equity data repository for spot prices
/// * `options_repo` - Options data repository for IV surfaces (CurrentMarketIV mode)
/// * `hedge_config` - Hedging configuration
/// * `timing` - Timing strategy for generating rehedge schedule
/// * `entry_time` - Trade entry time
/// * `exit_time` - Trade exit time
pub async fn apply_hedging<T, R>(
    trade: &T,
    result: &mut R,
    equity_repo: &dyn EquityDataRepository,
    _options_repo: &dyn OptionsDataRepository, // For future CurrentMarketIV support
    hedge_config: &HedgeConfig,
    timing: &TimingStrategy,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> Result<(), String>
where
    T: CompositeTrade + Clone + Send + Sync,
    R: TradeResult,
{
    // Skip if hedging is disabled
    if matches!(hedge_config.strategy, HedgeStrategy::None) {
        return Ok(());
    }

    // Skip if trade failed
    if !result.success() {
        return Ok(());
    }

    let symbol = result.symbol().to_string();
    let entry_spot = result.spot_at_entry();
    let exit_spot = result.spot_at_exit();

    // Generate rehedge schedule based on strategy
    let rehedge_times = timing.rehedge_times(entry_time, exit_time, &hedge_config.strategy);

    if rehedge_times.is_empty() {
        tracing::debug!(symbol = %symbol, "No rehedge times generated");
        return Ok(());
    }

    tracing::info!(
        symbol = %symbol,
        rehedge_count = rehedge_times.len(),
        strategy = ?hedge_config.strategy,
        delta_mode = ?hedge_config.delta_computation,
        "Applying hedging with trade structure"
    );

    // Execute hedging based on delta computation mode
    let hedge_position = match &hedge_config.delta_computation {
        DeltaComputation::GammaApproximation => {
            // Fast mode: use entry Greeks and gamma approximation
            let delta = result.net_delta().unwrap_or(0.0);
            let gamma = result.net_gamma().unwrap_or(0.0);

            if delta.abs() < 0.001 && gamma.abs() < 0.001 {
                tracing::warn!(
                    symbol = %symbol,
                    delta = delta,
                    gamma = gamma,
                    "Hedging enabled but Greeks are near-zero"
                );
            }

            let provider = GammaApproximationProvider::new(delta, gamma, entry_spot);
            execute_hedge_loop(
                hedge_config,
                provider,
                entry_spot,
                exit_spot,
                &symbol,
                equity_repo,
                rehedge_times,
                result.entry_iv().map(|iv| iv.primary),
                result.exit_iv().map(|iv| iv.primary),
            ).await?
        }

        DeltaComputation::EntryIV { .. } => {
            // Recompute delta from Black-Scholes using entry IV
            let entry_iv = result.entry_iv()
                .map(|iv| iv.primary)
                .ok_or("No entry IV available for EntryIV hedging mode")?;

            tracing::debug!(
                symbol = %symbol,
                entry_iv = entry_iv,
                "Using EntryIV delta provider"
            );

            let provider = EntryVolatilityProvider::new_entry_iv(trade.clone(), entry_iv, 0.05);
            execute_hedge_loop(
                hedge_config,
                provider,
                entry_spot,
                exit_spot,
                &symbol,
                equity_repo,
                rehedge_times,
                Some(entry_iv),
                result.exit_iv().map(|iv| iv.primary),
            ).await?
        }

        DeltaComputation::EntryHV { window } => {
            // Recompute delta from Black-Scholes using entry HV
            let entry_hv = compute_hv_at_time(equity_repo, &symbol, entry_time, *window).await?;

            tracing::debug!(
                symbol = %symbol,
                entry_hv = entry_hv,
                window = window,
                "Using EntryHV delta provider"
            );

            let provider = EntryVolatilityProvider::new_entry_hv(trade.clone(), entry_hv, 0.05);
            execute_hedge_loop(
                hedge_config,
                provider,
                entry_spot,
                exit_spot,
                &symbol,
                equity_repo,
                rehedge_times,
                result.entry_iv().map(|iv| iv.primary),
                result.exit_iv().map(|iv| iv.primary),
            ).await?
        }

        DeltaComputation::CurrentHV { window } => {
            // Recompute delta using current HV at each rehedge
            tracing::debug!(
                symbol = %symbol,
                window = window,
                "Using CurrentHV delta provider"
            );

            // CurrentHVProvider needs Arc, but we have &dyn - need to work around
            // For now, fall back to gamma approximation with warning
            tracing::warn!(
                symbol = %symbol,
                "CurrentHV mode requires Arc<EquityDataRepository>, falling back to GammaApproximation"
            );
            let delta = result.net_delta().unwrap_or(0.0);
            let gamma = result.net_gamma().unwrap_or(0.0);
            let provider = GammaApproximationProvider::new(delta, gamma, entry_spot);
            execute_hedge_loop(
                hedge_config,
                provider,
                entry_spot,
                exit_spot,
                &symbol,
                equity_repo,
                rehedge_times,
                result.entry_iv().map(|iv| iv.primary),
                result.exit_iv().map(|iv| iv.primary),
            ).await?
        }

        DeltaComputation::CurrentMarketIV { .. } => {
            // Rebuild IV surface at each rehedge (most accurate, most expensive)
            tracing::warn!(
                symbol = %symbol,
                "CurrentMarketIV mode requires Arc<OptionsDataRepository>, falling back to GammaApproximation"
            );
            let delta = result.net_delta().unwrap_or(0.0);
            let gamma = result.net_gamma().unwrap_or(0.0);
            let provider = GammaApproximationProvider::new(delta, gamma, entry_spot);
            execute_hedge_loop(
                hedge_config,
                provider,
                entry_spot,
                exit_spot,
                &symbol,
                equity_repo,
                rehedge_times,
                result.entry_iv().map(|iv| iv.primary),
                result.exit_iv().map(|iv| iv.primary),
            ).await?
        }

        DeltaComputation::HistoricalAverageIV { .. } => {
            // Use averaged IV over lookback period
            tracing::warn!(
                symbol = %symbol,
                "HistoricalAverageIV mode requires Arc repos, falling back to GammaApproximation"
            );
            let delta = result.net_delta().unwrap_or(0.0);
            let gamma = result.net_gamma().unwrap_or(0.0);
            let provider = GammaApproximationProvider::new(delta, gamma, entry_spot);
            execute_hedge_loop(
                hedge_config,
                provider,
                entry_spot,
                exit_spot,
                &symbol,
                equity_repo,
                rehedge_times,
                result.entry_iv().map(|iv| iv.primary),
                result.exit_iv().map(|iv| iv.primary),
            ).await?
        }
    };

    // Apply results if any hedging occurred
    if hedge_position.rehedge_count() > 0 {
        let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
        let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;

        tracing::info!(
            symbol = %symbol,
            rehedges = hedge_position.rehedge_count(),
            hedge_pnl = %hedge_pnl,
            total_cost = %hedge_position.total_cost,
            total_pnl = %total_pnl,
            "Applied hedge results"
        );

        result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, None);
    } else {
        tracing::debug!(symbol = %symbol, "No hedges executed");
    }

    Ok(())
}

/// Execute the hedging loop with a specific delta provider
async fn execute_hedge_loop<P: cs_domain::DeltaProvider>(
    hedge_config: &HedgeConfig,
    provider: P,
    entry_spot: f64,
    exit_spot: f64,
    symbol: &str,
    equity_repo: &dyn EquityDataRepository,
    rehedge_times: Vec<DateTime<Utc>>,
    entry_iv: Option<f64>,
    exit_iv: Option<f64>,
) -> Result<HedgePosition, String> {
    let mut hedge_state = GenericHedgeState::new(
        hedge_config.clone(),
        provider,
        entry_spot,
        false, // attribution not supported in strategy path yet
    );

    for rehedge_time in rehedge_times {
        if hedge_state.at_max_rehedges() {
            break;
        }

        let spot = equity_repo
            .get_spot_price(symbol, rehedge_time)
            .await
            .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?
            .to_f64();

        hedge_state.update(rehedge_time, spot).await?;
    }

    Ok(hedge_state.finalize(exit_spot, entry_iv, exit_iv))
}

/// Compute historical volatility at a specific time using intraday minute bars
async fn compute_hv_at_time(
    equity_repo: &dyn EquityDataRepository,
    symbol: &str,
    time: DateTime<Utc>,
    window: u32,
) -> Result<f64, String> {
    use cs_analytics::realized_volatility;

    let date = time.date_naive();

    let bars = equity_repo
        .get_bars(symbol, date)
        .await
        .map_err(|e| format!("Failed to get bars: {}", e))?;

    let closes: Vec<f64> = bars.column("close")
        .map_err(|_| "No close column".to_string())?
        .f64()
        .map_err(|_| "Invalid close type".to_string())?
        .into_no_null_iter()
        .collect();

    realized_volatility(&closes, window as usize, 252.0)
        .ok_or_else(|| "Insufficient data for HV computation".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_domain::StraddleResult;

    #[test]
    fn test_straddle_result_implements_trade_result() {
        fn assert_trade_result<T: TradeResult>() {}
        assert_trade_result::<StraddleResult>();
    }
}
