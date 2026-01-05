//! Helper methods for BacktestUseCase - selector + executor composition

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use tracing::warn;
use cs_domain::*;
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria};
use crate::execution::{execute_trade, ExecutionConfig};
use crate::spread_pricer::SpreadPricer;
use crate::straddle_pricer::StraddlePricer;
use crate::iron_butterfly_pricer::IronButterflyPricer;
use crate::calendar_straddle_pricer::CalendarStraddlePricer;
use crate::iv_surface_builder::build_iv_surface_minute_aligned;
use finq_core::OptionType;

/// Helper to execute calendar spread: select + execute
pub async fn execute_calendar_spread(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    option_type: OptionType,
    config: &ExecutionConfig,
) -> Option<CalendarSpreadResult> {
    // Get option chain
    let entry_chain = options_repo
        .get_option_bars_at_time(&event.symbol, entry_time)
        .await
        .ok()?;

    // Build IV surface
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        &event.symbol,
    ).await?;

    // Get spot price
    let spot = equity_repo
        .get_spot_price(&event.symbol, entry_time)
        .await
        .ok()?;

    // SELECT trade
    let spread = selector
        .select_calendar_spread(
            &spot,
            &entry_surface,
            option_type,
            criteria,
        )
        .ok()?;

    // EXECUTE trade
    let pricer = SpreadPricer::new();
    let result = execute_trade(
        &spread,
        &pricer,
        options_repo,
        equity_repo,
        config,
        event,
        entry_time,
        exit_time,
    ).await;

    Some(result)
}

/// Helper to execute straddle: select + execute
pub async fn execute_straddle(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    config: &ExecutionConfig,
) -> Option<StraddleResult> {
    // Get option chain
    let entry_chain = options_repo
        .get_option_bars_at_time(&event.symbol, entry_time)
        .await
        .ok()?;

    // Build IV surface
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        &event.symbol,
    ).await?;

    // Get spot price
    let spot = equity_repo
        .get_spot_price(&event.symbol, entry_time)
        .await
        .ok()?;

    // Calculate min expiration from entry time
    let min_expiration = (entry_time.date_naive() + chrono::Duration::days(criteria.min_short_dte as i64))
        .max(entry_time.date_naive());

    // SELECT trade
    let straddle = selector
        .select_straddle(
            &spot,
            &entry_surface,
            min_expiration,
        )
        .ok()?;

    // EXECUTE trade
    let spread_pricer = SpreadPricer::new();
    let pricer = StraddlePricer::new(spread_pricer);
    let result = execute_trade(
        &straddle,
        &pricer,
        options_repo,
        equity_repo,
        config,
        event,
        entry_time,
        exit_time,
    ).await;

    Some(result)
}

/// Helper to execute iron butterfly: select + execute
pub async fn execute_iron_butterfly(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    config: &ExecutionConfig,
) -> Option<IronButterflyResult> {
    // Get option chain
    let entry_chain = options_repo
        .get_option_bars_at_time(&event.symbol, entry_time)
        .await
        .ok()?;

    // Build IV surface
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        &event.symbol,
    ).await?;

    // Get spot price
    let spot = equity_repo
        .get_spot_price(&event.symbol, entry_time)
        .await
        .ok()?;

    // Get wing width from config (need to pass through ExecutionConfig)
    // For now, use default of 5
    let wing_width = Decimal::new(5, 0);

    // SELECT trade
    let iron_butterfly = selector
        .select_iron_butterfly(
            &spot,
            &entry_surface,
            wing_width,
            criteria.min_short_dte,
            criteria.max_short_dte,
        )
        .ok()?;

    // EXECUTE trade
    let spread_pricer = SpreadPricer::new();
    let pricer = IronButterflyPricer::new(spread_pricer);
    let result = execute_trade(
        &iron_butterfly,
        &pricer,
        options_repo,
        equity_repo,
        config,
        event,
        entry_time,
        exit_time,
    ).await;

    Some(result)
}

/// Helper to execute calendar straddle: select + execute
pub async fn execute_calendar_straddle(
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    selector: &dyn StrikeSelector,
    criteria: &ExpirationCriteria,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    config: &ExecutionConfig,
) -> Option<CalendarStraddleResult> {
    // Get option chain
    let entry_chain = options_repo
        .get_option_bars_at_time(&event.symbol, entry_time)
        .await
        .ok()?;

    // Build IV surface
    let entry_surface = build_iv_surface_minute_aligned(
        &entry_chain,
        equity_repo,
        &event.symbol,
    ).await?;

    // Get spot price
    let spot = equity_repo
        .get_spot_price(&event.symbol, entry_time)
        .await
        .ok()?;

    // SELECT trade
    let calendar_straddle = selector
        .select_calendar_straddle(
            &spot,
            &entry_surface,
            criteria,
        )
        .ok()?;

    // EXECUTE trade
    let spread_pricer = SpreadPricer::new();
    let pricer = CalendarStraddlePricer::new(spread_pricer);
    let result = execute_trade(
        &calendar_straddle,
        &pricer,
        options_repo,
        equity_repo,
        config,
        event,
        entry_time,
        exit_time,
    ).await;

    Some(result)
}
