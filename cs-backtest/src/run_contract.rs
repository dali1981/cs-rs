//! Canonical run contract types for backtest execution.
//!
//! These types are documentation-facing domain contracts that make the expected
//! run inputs and outputs explicit and portable across callers.

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::{
    BacktestPeriod, BacktestResult, DataSourceConfig, EarningsSourceConfig, RunBacktestCommand,
    SelectionType, SpreadType, TradeResultMethods, UnifiedBacktestResult,
};
use cs_domain::ReturnBasis;

/// Explicit run input contract for a canonical backtest execution.
#[derive(Debug, Clone)]
pub struct RunInput {
    pub command: RunBacktestCommand,
    pub data_source: DataSourceConfig,
    pub earnings_source: EarningsSourceConfig,
}

impl RunInput {
    pub fn new(
        command: RunBacktestCommand,
        data_source: DataSourceConfig,
        earnings_source: EarningsSourceConfig,
    ) -> Self {
        Self {
            command,
            data_source,
            earnings_source,
        }
    }
}

/// Coarse-grained strategy family used in summaries and reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyFamily {
    CalendarSpread,
    IronButterfly,
    Straddle,
    CalendarStraddle,
    PostEarningsStraddle,
}

impl StrategyFamily {
    pub fn from_spread(spread: SpreadType) -> Self {
        match spread {
            SpreadType::Calendar => Self::CalendarSpread,
            SpreadType::IronButterfly | SpreadType::LongIronButterfly => Self::IronButterfly,
            SpreadType::Straddle | SpreadType::ShortStraddle => Self::Straddle,
            SpreadType::CalendarStraddle => Self::CalendarStraddle,
            SpreadType::PostEarningsStraddle => Self::PostEarningsStraddle,
        }
    }
}

/// Required summary for every completed run.
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub strategy_family: StrategyFamily,
    pub strategy: SpreadType,
    pub selection_strategy: SelectionType,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub sessions_processed: usize,
    pub total_opportunities: usize,
    pub trade_count: usize,
    pub dropped_event_count: usize,
    pub win_rate_pct: f64,
    pub total_pnl: Decimal,
    pub hedging_enabled: bool,
    pub total_hedge_pnl: Option<Decimal>,
    pub total_pnl_with_hedge: Option<Decimal>,
    pub return_basis: ReturnBasis,
}

impl RunSummary {
    pub fn from_backtest_result<R>(
        strategy_family: StrategyFamily,
        strategy: SpreadType,
        selection_strategy: SelectionType,
        period: &BacktestPeriod,
        return_basis: ReturnBasis,
        result: &BacktestResult<R>,
    ) -> Self
    where
        R: TradeResultMethods,
    {
        let hedging_enabled = result.has_hedging();

        Self {
            strategy_family,
            strategy,
            selection_strategy,
            start_date: period.start_date,
            end_date: period.end_date,
            sessions_processed: result.sessions_processed,
            total_opportunities: result.total_opportunities,
            trade_count: result.successful_trades(),
            dropped_event_count: result.dropped_events.len(),
            win_rate_pct: result.win_rate() * 100.0,
            total_pnl: result.total_pnl(),
            hedging_enabled,
            total_hedge_pnl: hedging_enabled.then_some(result.total_hedge_pnl()),
            total_pnl_with_hedge: hedging_enabled.then_some(result.total_pnl_with_hedge()),
            return_basis,
        }
    }
}

/// Explicit run output contract for a completed canonical backtest.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub input: RunInput,
    pub summary: RunSummary,
}

impl RunOutput {
    pub fn from_result(input: RunInput, result: &UnifiedBacktestResult) -> Self {
        let summary = match result {
            UnifiedBacktestResult::CalendarSpread(r) => RunSummary::from_backtest_result(
                StrategyFamily::CalendarSpread,
                input.command.strategy.spread,
                input.command.strategy.selection_strategy,
                &input.command.period,
                input.command.risk.return_basis,
                r,
            ),
            UnifiedBacktestResult::IronButterfly(r) => RunSummary::from_backtest_result(
                StrategyFamily::IronButterfly,
                input.command.strategy.spread,
                input.command.strategy.selection_strategy,
                &input.command.period,
                input.command.risk.return_basis,
                r,
            ),
            UnifiedBacktestResult::Straddle(r) => RunSummary::from_backtest_result(
                StrategyFamily::Straddle,
                input.command.strategy.spread,
                input.command.strategy.selection_strategy,
                &input.command.period,
                input.command.risk.return_basis,
                r,
            ),
            UnifiedBacktestResult::CalendarStraddle(r) => RunSummary::from_backtest_result(
                StrategyFamily::CalendarStraddle,
                input.command.strategy.spread,
                input.command.strategy.selection_strategy,
                &input.command.period,
                input.command.risk.return_basis,
                r,
            ),
            UnifiedBacktestResult::PostEarningsStraddle(r) => RunSummary::from_backtest_result(
                StrategyFamily::PostEarningsStraddle,
                input.command.strategy.spread,
                input.command.strategy.selection_strategy,
                &input.command.period,
                input.command.risk.return_basis,
                r,
            ),
        };

        Self { input, summary }
    }
}
