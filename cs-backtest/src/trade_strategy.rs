//! Trade Strategy Pattern for Backtest Execution
//!
//! This module provides a Strategy pattern abstraction for executing different
//! trade types (calendar spreads, straddles, iron butterflies, etc.) with a
//! unified interface.
//!
//! # Architecture
//!
//! ```text
//! BacktestUseCase::execute()
//!     │
//!     ├── create_strategy(SpreadType) → Box<dyn TradeStrategy<R>>
//!     │
//!     └── execute_with_strategy(strategy)
//!             │
//!             ├── iterate session dates
//!             ├── load earnings events
//!             ├── filter for entry
//!             └── execute_batch (parallel or sequential)
//!                     │
//!                     └── strategy.execute_trade(...)
//! ```
//!
//! # Adding a New Strategy
//!
//! 1. Create a struct implementing `TradeStrategy<YourResultType>`
//! 2. Add a variant to `SpreadType` in config.rs
//! 3. Update `strategy_factory::create_strategy()` to handle the new variant

use std::future::Future;
use std::pin::Pin;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use cs_domain::*;
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria};
use crate::config::BacktestConfig;
use crate::execution::ExecutionConfig;
use crate::timing_strategy::TimingStrategy;
use crate::backtest_use_case::{TradeResultMethods, TradeGenerationError};
use crate::backtest_use_case_helpers::TradeExecutionContext;
use crate::composite_pricer::{
    CompositePricer, CalendarSpreadPricer, IronButterflyCompositePricer,
    CalendarStraddleCompositePricer,
};

/// Core trait for trade execution strategies
///
/// Each strategy encapsulates:
/// - Timing logic (when to enter/exit)
/// - Execution configuration (validation thresholds)
/// - Trade execution logic (how to execute the specific trade type)
/// - Result filtering (IV ratio filters, etc.)
pub trait TradeStrategy<R: TradeResultMethods + Send>: Send + Sync {
    /// Get the timing strategy for this trade type
    fn timing(&self) -> &TimingStrategy;

    /// Get the execution configuration
    fn execution_config(&self) -> &ExecutionConfig;

    /// Execute a single trade
    ///
    /// Returns `None` if the trade could not be executed (missing data, validation failure, etc.)
    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<R>> + Send + 'a>>;

    /// Apply post-execution filters to the result
    ///
    /// Returns `true` if the result passes all filters, `false` if it should be dropped.
    /// Default implementation passes all results.
    fn apply_filter(&self, _result: &R) -> bool {
        true
    }

    /// Create a filter rejection error for dropped trades
    ///
    /// Called when `apply_filter` returns `false` to create an error record.
    fn create_filter_error(&self, result: &R, event: &EarningsEvent) -> Option<TradeGenerationError> {
        let _ = (result, event);
        None
    }

    /// Calculate lookahead days for earnings loading
    ///
    /// Different strategies need different lookahead windows based on their timing.
    fn lookahead_days(&self) -> i64 {
        self.timing().lookahead_days()
    }

    /// Get the entry date for an event
    fn entry_date(&self, event: &EarningsEvent) -> NaiveDate {
        self.timing().entry_date(event)
    }

    /// Get entry datetime for an event
    fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        self.timing().entry_datetime(event)
    }

    /// Get exit datetime for an event
    fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc> {
        self.timing().exit_datetime(event)
    }
}

// ============================================================================
// Concrete Strategy Implementations
// ============================================================================

/// Calendar Spread Strategy
pub struct CalendarSpreadStrategy {
    timing: TimingStrategy,
    exec_config: ExecutionConfig,
    option_type: finq_core::OptionType,
    min_iv_ratio: Option<f64>,
}

impl CalendarSpreadStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::Earnings(
            EarningsTradeTiming::new(config.timing)
        );
        let exec_config = ExecutionConfig::for_calendar_spread(config.max_entry_iv);
        Self {
            timing,
            exec_config,
            option_type: finq_core::OptionType::Call, // Default to calls
            min_iv_ratio: config.selection.min_iv_ratio,
        }
    }

    pub fn with_option_type(mut self, option_type: finq_core::OptionType) -> Self {
        self.option_type = option_type;
        self
    }
}

impl TradeStrategy<CalendarSpreadResult> for CalendarSpreadStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execution_config(&self) -> &ExecutionConfig {
        &self.exec_config
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<CalendarSpreadResult>> + Send + 'a>> {
        let option_type = self.option_type;
        Box::pin(async move {
            let ctx = TradeExecutionContext::new(
                options_repo, equity_repo, event, entry_time, exit_time, &self.exec_config,
            );
            ctx.execute(
                |data| selector.select_calendar_spread(&data.spot, &data.surface, option_type, criteria).ok(),
                CalendarSpreadPricer::new(),
            ).await
        })
    }

    fn apply_filter(&self, result: &CalendarSpreadResult) -> bool {
        match (self.min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn create_filter_error(&self, result: &CalendarSpreadResult, _event: &EarningsEvent) -> Option<TradeGenerationError> {
        Some(TradeGenerationError {
            symbol: result.symbol.clone(),
            earnings_date: result.earnings_date,
            earnings_time: result.earnings_time,
            reason: "IV_RATIO_FILTER".into(),
            details: result.iv_ratio().map(|r| format!("IV ratio: {:.2}", r)),
            phase: "filter".into(),
        })
    }
}

/// Iron Butterfly Strategy
pub struct IronButterflyStrategy {
    timing: TimingStrategy,
    exec_config: ExecutionConfig,
    wing_width: Decimal,
}

impl IronButterflyStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::Earnings(
            EarningsTradeTiming::new(config.timing)
        );
        let exec_config = ExecutionConfig::for_iron_butterfly(config.max_entry_iv);
        Self {
            timing,
            exec_config,
            wing_width: Decimal::new(5, 0), // Default wing width
        }
    }

    pub fn with_wing_width(mut self, wing_width: Decimal) -> Self {
        self.wing_width = wing_width;
        self
    }
}

impl TradeStrategy<IronButterflyResult> for IronButterflyStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execution_config(&self) -> &ExecutionConfig {
        &self.exec_config
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<IronButterflyResult>> + Send + 'a>> {
        let wing_width = self.wing_width;
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        Box::pin(async move {
            let ctx = TradeExecutionContext::new(
                options_repo, equity_repo, event, entry_time, exit_time, &self.exec_config,
            );
            ctx.execute(
                |data| selector.select_iron_butterfly(
                    &data.spot,
                    &data.surface,
                    wing_width,
                    min_short_dte,
                    max_short_dte,
                ).ok(),
                IronButterflyCompositePricer::new(),
            ).await
        })
    }
}

/// Straddle Strategy (pre-earnings)
pub struct StraddleStrategy {
    timing: TimingStrategy,
    exec_config: ExecutionConfig,
}

impl StraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing_impl = StraddleTradeTiming::new(config.timing)
            .with_entry_days(config.straddle_entry_days)
            .with_exit_days(config.straddle_exit_days);
        let timing = TimingStrategy::Straddle(timing_impl);
        let exec_config = ExecutionConfig::for_straddle(config.max_entry_iv);
        Self { timing, exec_config }
    }
}

impl TradeStrategy<StraddleResult> for StraddleStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execution_config(&self) -> &ExecutionConfig {
        &self.exec_config
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        Box::pin(async move {
            let ctx = TradeExecutionContext::new(
                options_repo, equity_repo, event, entry_time, exit_time, &self.exec_config,
            );
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            ctx.execute(
                |data| selector.select_straddle(&data.spot, &data.surface, min_expiration).ok(),
                CompositePricer::default(),
            ).await
        })
    }
}

/// Post-Earnings Straddle Strategy
pub struct PostEarningsStraddleStrategy {
    timing: TimingStrategy,
    exec_config: ExecutionConfig,
}

impl PostEarningsStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing_impl = PostEarningsStraddleTiming::new(config.timing)
            .with_holding_days(config.post_earnings_holding_days);
        let timing = TimingStrategy::PostEarnings(timing_impl);
        let exec_config = ExecutionConfig::for_straddle(config.max_entry_iv);
        Self { timing, exec_config }
    }
}

impl TradeStrategy<StraddleResult> for PostEarningsStraddleStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execution_config(&self) -> &ExecutionConfig {
        &self.exec_config
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        Box::pin(async move {
            let ctx = TradeExecutionContext::new(
                options_repo, equity_repo, event, entry_time, exit_time, &self.exec_config,
            );
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            ctx.execute(
                |data| selector.select_straddle(&data.spot, &data.surface, min_expiration).ok(),
                CompositePricer::default(),
            ).await
        })
    }
}

/// Calendar Straddle Strategy
pub struct CalendarStraddleStrategy {
    timing: TimingStrategy,
    exec_config: ExecutionConfig,
    min_iv_ratio: Option<f64>,
}

impl CalendarStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::Earnings(
            EarningsTradeTiming::new(config.timing)
        );
        let exec_config = ExecutionConfig::for_calendar_straddle(config.max_entry_iv);
        Self {
            timing,
            exec_config,
            min_iv_ratio: config.selection.min_iv_ratio,
        }
    }
}

impl TradeStrategy<CalendarStraddleResult> for CalendarStraddleStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execution_config(&self) -> &ExecutionConfig {
        &self.exec_config
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<CalendarStraddleResult>> + Send + 'a>> {
        Box::pin(async move {
            let ctx = TradeExecutionContext::new(
                options_repo, equity_repo, event, entry_time, exit_time, &self.exec_config,
            );
            ctx.execute(
                |data| selector.select_calendar_straddle(&data.spot, &data.surface, criteria).ok(),
                CalendarStraddleCompositePricer::new(),
            ).await
        })
    }

    fn apply_filter(&self, result: &CalendarStraddleResult) -> bool {
        match (self.min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn create_filter_error(&self, result: &CalendarStraddleResult, _event: &EarningsEvent) -> Option<TradeGenerationError> {
        Some(TradeGenerationError {
            symbol: result.symbol.clone(),
            earnings_date: result.earnings_date,
            earnings_time: result.earnings_time,
            reason: "IV_RATIO_FILTER".into(),
            details: result.iv_ratio().map(|r| format!("IV ratio: {:.2}", r)),
            phase: "filter".into(),
        })
    }
}

// ============================================================================
// Strategy Factory
// ============================================================================

use crate::config::SpreadType;

/// Enum wrapper for type-erased strategy dispatch
///
/// This allows `execute()` to work with different result types through
/// the `UnifiedBacktestResult` enum.
pub enum StrategyDispatch {
    CalendarSpread(CalendarSpreadStrategy),
    IronButterfly(IronButterflyStrategy),
    Straddle(StraddleStrategy),
    PostEarningsStraddle(PostEarningsStraddleStrategy),
    CalendarStraddle(CalendarStraddleStrategy),
}

impl StrategyDispatch {
    /// Create a strategy from SpreadType and config
    pub fn from_config(spread_type: SpreadType, config: &BacktestConfig) -> Self {
        match spread_type {
            SpreadType::Calendar => {
                StrategyDispatch::CalendarSpread(CalendarSpreadStrategy::new(config))
            }
            SpreadType::IronButterfly => {
                StrategyDispatch::IronButterfly(IronButterflyStrategy::new(config))
            }
            SpreadType::Straddle => {
                StrategyDispatch::Straddle(StraddleStrategy::new(config))
            }
            SpreadType::PostEarningsStraddle => {
                StrategyDispatch::PostEarningsStraddle(PostEarningsStraddleStrategy::new(config))
            }
            SpreadType::CalendarStraddle => {
                StrategyDispatch::CalendarStraddle(CalendarStraddleStrategy::new(config))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BacktestConfig;

    #[test]
    fn test_strategy_factory() {
        let config = BacktestConfig::default();

        let strategy = StrategyDispatch::from_config(SpreadType::Calendar, &config);
        assert!(matches!(strategy, StrategyDispatch::CalendarSpread(_)));

        let strategy = StrategyDispatch::from_config(SpreadType::Straddle, &config);
        assert!(matches!(strategy, StrategyDispatch::Straddle(_)));
    }
}
