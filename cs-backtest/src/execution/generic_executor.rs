//! Generic trade execution function

use chrono::{DateTime, Utc};
use cs_domain::{EquityDataRepository, OptionsDataRepository, EarningsEvent};
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use crate::trade_executor::ExecutionError;
use super::traits::{ExecutableTrade, TradePricer};
use super::types::{ExecutionConfig, ExecutionContext};

/// Execute any trade generically
///
/// This function implements the common 6-step execution pattern:
/// 1. Get spot prices (entry + exit)
/// 2. Get option chains (entry + exit)
/// 3. Build IV surfaces
/// 4. Price at entry
/// 5. Validate entry
/// 6. Price at exit
/// 7. Construct result
///
/// The trade type determines pricing, validation, and result construction via the ExecutableTrade trait.
pub async fn execute_trade<T>(
    trade: &T,
    pricer: &T::Pricer,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    config: &ExecutionConfig,
    earnings_event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> T::Result
where
    T: ExecutableTrade,
{
    match try_execute_trade(
        trade,
        pricer,
        options_repo,
        equity_repo,
        config,
        earnings_event,
        entry_time,
        exit_time,
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            // Create minimal context for failed result
            let ctx = ExecutionContext::new(
                entry_time,
                exit_time,
                0.0,
                0.0,
                None,
                entry_time, // dummy
                earnings_event,
            );
            T::to_failed_result(trade, &ctx, e)
        }
    }
}

async fn try_execute_trade<T>(
    trade: &T,
    pricer: &T::Pricer,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    config: &ExecutionConfig,
    earnings_event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> Result<T::Result, ExecutionError>
where
    T: ExecutableTrade,
{
    // 1. Get spot prices
    let entry_spot = equity_repo
        .get_spot_price(trade.symbol(), entry_time)
        .await?;
    let exit_spot = equity_repo
        .get_spot_price(trade.symbol(), exit_time)
        .await?;

    // 2. Get option chains
    let entry_chain = options_repo
        .get_option_bars_at_time(trade.symbol(), entry_time)
        .await?;
    let (exit_chain, exit_surface_time) = options_repo
        .get_option_bars_at_or_after_time(trade.symbol(), exit_time, 30)
        .await?;

    // 3. Build IV surfaces with per-option spot prices (minute-aligned)
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        trade.symbol(),
    )
    .await;
    let entry_surface_time = entry_surface.as_ref().map(|s| s.as_of_time());

    let exit_surface = build_iv_surface_minute_aligned(
        &exit_chain,
        equity_repo,
        trade.symbol(),
    )
    .await;

    // 4. Price at entry
    let entry_pricing = pricer.price_with_surface(
        trade,
        &entry_chain,
        entry_spot.to_f64(),
        entry_time,
        entry_surface.as_ref(),
    )?;

    // 5. Validate entry
    T::validate_entry(&entry_pricing, config)?;

    // 6. Price at exit
    let exit_pricing = pricer.price_with_surface(
        trade,
        &exit_chain,
        exit_spot.to_f64(),
        exit_time,
        exit_surface.as_ref(),
    )?;

    // 7. Construct result
    let ctx = ExecutionContext::new(
        entry_time,
        exit_time,
        entry_spot.to_f64(),
        exit_spot.to_f64(),
        entry_surface_time,
        exit_surface_time,
        earnings_event,
    );

    Ok(T::to_result(trade, entry_pricing, exit_pricing, &ctx))
}
