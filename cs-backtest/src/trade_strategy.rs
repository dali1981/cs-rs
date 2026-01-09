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
use crate::execution::{ExecutionConfig, ExecutableTrade};
use crate::timing_strategy::TimingStrategy;
use crate::backtest_use_case::{TradeResultMethods, TradeGenerationError};
use crate::backtest_use_case_helpers::TradeSimulator;
use crate::composite_pricer::{
    CompositePricer, CalendarSpreadPricer, IronButterflyCompositePricer,
    CalendarStraddleCompositePricer,
};

/// Core trait for trade execution strategies
///
/// Each strategy encapsulates:
/// - Timing logic (when to enter/exit)
/// - Trade execution logic (how to execute the specific trade type)
/// - Result filtering (IV ratio filters, etc.)
///
/// Validation config (ExecutionConfig) is passed to execute_trade, not owned by strategy.
pub trait TradeStrategy<R: TradeResultMethods + Send>: Send + Sync {
    /// Get the timing strategy for this trade type
    fn timing(&self) -> &TimingStrategy;

    /// Execute a single trade
    ///
    /// Returns `None` if the trade could not be executed (missing data, validation failure, etc.)
    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<R>> + Send + 'a>>;

    /// Apply post-execution filters to the result
    ///
    /// Returns `true` if the result passes all filters, `false` if it should be dropped.
    /// Default implementation passes all results.
    ///
    /// `min_iv_ratio` is passed from config to enable IV ratio filtering without
    /// storing filter config in strategy structs.
    fn apply_filter(&self, _result: &R, _min_iv_ratio: Option<f64>) -> bool {
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
    option_type: finq_core::OptionType,
}

impl CalendarSpreadStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        Self {
            timing,
            option_type: finq_core::OptionType::Call, // Default to calls
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

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<CalendarSpreadResult>> + Send + 'a>> {
        let option_type = self.option_type;
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = simulator.prepare().await?;

            // 2. Select trade
            let trade = selector.select_calendar_spread(&data.spot, &data.surface, option_type, criteria).ok()?;

            // 3. Simulate and convert to result
            let pricer = CalendarSpreadPricer::new();
            let result = match simulator.run(&trade, &pricer).await {
                Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event)),
                Err(err) => trade.to_failed_result(&simulator.failed_output(), Some(event), err),
            };

            Some(result)
        })
    }

    fn apply_filter(&self, result: &CalendarSpreadResult, min_iv_ratio: Option<f64>) -> bool {
        match (min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn create_filter_error(&self, result: &CalendarSpreadResult, event: &EarningsEvent) -> Option<TradeGenerationError> {
        Some(TradeGenerationError {
            symbol: result.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            reason: "IV_RATIO_FILTER".into(),
            details: result.iv_ratio().map(|r| format!("IV ratio: {:.2}", r)),
            phase: "filter".into(),
        })
    }
}

/// Iron Butterfly Strategy
pub struct IronButterflyStrategy {
    timing: TimingStrategy,
    wing_width: Decimal,
}

impl IronButterflyStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        Self {
            timing,
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

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<IronButterflyResult>> + Send + 'a>> {
        let wing_width = self.wing_width;
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = simulator.prepare().await?;

            // 2. Select trade
            let trade = selector.select_iron_butterfly(
                &data.spot,
                &data.surface,
                wing_width,
                min_short_dte,
                max_short_dte,
            ).ok()?;

            // 3. Simulate and convert to result
            let pricer = IronButterflyCompositePricer::new();
            let result = match simulator.run(&trade, &pricer).await {
                Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event)),
                Err(err) => trade.to_failed_result(&simulator.failed_output(), Some(event), err),
            };

            Some(result)
        })
    }
}

/// Straddle Strategy (pre-earnings)
pub struct StraddleStrategy {
    timing: TimingStrategy,
}

impl StraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_straddle(
            config.timing,
            config.straddle_entry_days,
            config.straddle_exit_days,
        );
        Self { timing }
    }
}

impl TradeStrategy<StraddleResult> for StraddleStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = simulator.prepare().await?;

            // 2. Select trade
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            let trade = selector.select_straddle(&data.spot, &data.surface, min_expiration).ok()?;

            // 3. Simulate and convert to result
            let pricer = CompositePricer::default();
            let result = match simulator.run(&trade, &pricer).await {
                Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event)),
                Err(err) => trade.to_failed_result(&simulator.failed_output(), Some(event), err),
            };

            Some(result)
        })
    }
}

/// Post-Earnings Straddle Strategy
pub struct PostEarningsStraddleStrategy {
    timing: TimingStrategy,
}

impl PostEarningsStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_post_earnings(
            config.timing,
            config.post_earnings_holding_days,
        );
        Self { timing }
    }
}

impl TradeStrategy<StraddleResult> for PostEarningsStraddleStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = simulator.prepare().await?;

            // 2. Select trade
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            let trade = selector.select_straddle(&data.spot, &data.surface, min_expiration).ok()?;

            // 3. Simulate and convert to result
            let pricer = CompositePricer::default();
            let result = match simulator.run(&trade, &pricer).await {
                Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event)),
                Err(err) => trade.to_failed_result(&simulator.failed_output(), Some(event), err),
            };

            Some(result)
        })
    }
}

/// Calendar Straddle Strategy
pub struct CalendarStraddleStrategy {
    timing: TimingStrategy,
}

impl CalendarStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        Self { timing }
    }
}

impl TradeStrategy<CalendarStraddleResult> for CalendarStraddleStrategy {
    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn execute_trade<'a>(
        &'a self,
        options_repo: &'a dyn OptionsDataRepository,
        equity_repo: &'a dyn EquityDataRepository,
        selector: &'a dyn StrikeSelector,
        criteria: &'a ExpirationCriteria,
        exec_config: &'a ExecutionConfig,
        event: &'a EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = Option<CalendarStraddleResult>> + Send + 'a>> {
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = simulator.prepare().await?;

            // 2. Select trade
            let trade = selector.select_calendar_straddle(&data.spot, &data.surface, criteria).ok()?;

            // 3. Simulate and convert to result
            let pricer = CalendarStraddleCompositePricer::new();
            let result = match simulator.run(&trade, &pricer).await {
                Ok(raw) => trade.to_result(raw.entry_pricing, raw.exit_pricing, &raw.output, Some(event)),
                Err(err) => trade.to_failed_result(&simulator.failed_output(), Some(event), err),
            };

            Some(result)
        })
    }

    fn apply_filter(&self, result: &CalendarStraddleResult, min_iv_ratio: Option<f64>) -> bool {
        match (min_iv_ratio, result.iv_ratio()) {
            (Some(min), Some(ratio)) => ratio >= min,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    fn create_filter_error(&self, result: &CalendarStraddleResult, event: &EarningsEvent) -> Option<TradeGenerationError> {
        Some(TradeGenerationError {
            symbol: result.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
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
