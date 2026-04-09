//! Canonical run contract types for backtest execution.
//!
//! These types are documentation-facing domain contracts that make the expected
//! run inputs and outputs explicit and portable across callers.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::fmt;

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

impl fmt::Display for StrategyFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CalendarSpread => write!(f, "calendar-spread"),
            Self::IronButterfly => write!(f, "iron-butterfly"),
            Self::Straddle => write!(f, "straddle"),
            Self::CalendarStraddle => write!(f, "calendar-straddle"),
            Self::PostEarningsStraddle => write!(f, "post-earnings-straddle"),
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
    pub total_entries: usize,
    pub total_opportunities: usize,
    pub trade_count: usize,
    pub dropped_event_count: usize,
    pub win_rate_pct: Decimal,
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
        let winners = result.results.iter().filter(|r| r.is_winner()).count();
        let successful_trades = result.successful_trades();
        let win_rate_pct = if successful_trades == 0 {
            Decimal::ZERO
        } else {
            (Decimal::from(winners as u64) * Decimal::from(100u64))
                / Decimal::from(successful_trades as u64)
        };
        let hedging_enabled = result.has_hedging();

        Self {
            strategy_family,
            strategy,
            selection_strategy,
            start_date: period.start_date,
            end_date: period.end_date,
            sessions_processed: result.sessions_processed,
            total_entries: result.total_entries,
            total_opportunities: result.total_opportunities,
            trade_count: successful_trades,
            dropped_event_count: result.dropped_events.len(),
            win_rate_pct,
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
        let strategy = input.command.strategy.spread;
        let strategy_family = StrategyFamily::from_spread(strategy);
        let selection_strategy = input.command.strategy.selection_strategy;
        let period = &input.command.period;
        let return_basis = input.command.risk.return_basis;

        let summary = match result {
            UnifiedBacktestResult::CalendarSpread(r) => RunSummary::from_backtest_result(
                strategy_family,
                strategy,
                selection_strategy,
                period,
                return_basis,
                r,
            ),
            UnifiedBacktestResult::IronButterfly(r) => RunSummary::from_backtest_result(
                strategy_family,
                strategy,
                selection_strategy,
                period,
                return_basis,
                r,
            ),
            UnifiedBacktestResult::Straddle(r) => RunSummary::from_backtest_result(
                strategy_family,
                strategy,
                selection_strategy,
                period,
                return_basis,
                r,
            ),
            UnifiedBacktestResult::CalendarStraddle(r) => RunSummary::from_backtest_result(
                strategy_family,
                strategy,
                selection_strategy,
                period,
                return_basis,
                r,
            ),
            UnifiedBacktestResult::PostEarningsStraddle(r) => RunSummary::from_backtest_result(
                strategy_family,
                strategy,
                selection_strategy,
                period,
                return_basis,
                r,
            ),
        };

        Self { input, summary }
    }
}
