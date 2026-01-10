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
use rust_decimal::prelude::ToPrimitive;
use cs_domain::*;
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria};
use crate::config::BacktestConfig;
use crate::bpr::{build_bpr_timeline, BprPricingContext, HasBprTimeline};
use crate::execution::{ExecutionConfig, ExecutableTrade, TradePricer};
use crate::timing_strategy::TimingStrategy;
use crate::backtest_use_case::{TradeResultMethods, TradeGenerationError};
use crate::backtest_use_case_helpers::{PreparedData, TradeSimulator};
use crate::hedging_simulator::{simulate_with_hedging_prepriced, EntryPricingContext, HedgedSimulationOutput};
use crate::composite_pricer::{
    CompositePricer, CompositePricing, CalendarSpreadPricer, IronButterflyCompositePricer,
    LongIronButterflyCompositePricer, CalendarStraddleCompositePricer, ShortStraddlePricer,
};
use crate::rules::RuleEvaluator;

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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<R>> + Send + 'a>>;

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

pub enum TradeExecutionOutcome<R> {
    Executed(R),
    Dropped(TradeGenerationError),
    Skipped,
}

fn passes_market_rules(
    rule_evaluator: &RuleEvaluator,
    event: &EarningsEvent,
    data: &PreparedData,
) -> Result<(), TradeGenerationError> {
    if !rule_evaluator.has_market_rules() {
        return Ok(());
    }

    match rule_evaluator.eval_market_rules_with_reason(event, data) {
        Ok(None) => Ok(()),
        Ok(Some(rule_name)) => Err(TradeGenerationError {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            reason: "MARKET_RULE_FAILED".into(),
            details: Some(format!("Rule: {}", rule_name)),
            phase: "filter".into(),
        }),
        Err(e) => Err(TradeGenerationError {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            reason: "MARKET_RULE_ERROR".into(),
            details: Some(e.to_string()),
            phase: "filter".into(),
        }),
    }
}

fn passes_trade_rules(
    rule_evaluator: &RuleEvaluator,
    event: &EarningsEvent,
    entry_price: f64,
) -> Result<(), TradeGenerationError> {
    if !rule_evaluator.has_trade_rules() {
        return Ok(());
    }

    match rule_evaluator.eval_trade_rules_with_reason(event, entry_price) {
        None => Ok(()),
        Some(rule_name) => Err(TradeGenerationError {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            reason: "TRADE_RULE_FAILED".into(),
            details: Some(format!("Rule: {}, entry_price: {:.4}", rule_name, entry_price)),
            phase: "filter".into(),
        }),
    }
}

fn entry_price_per_contract(pricing: &CompositePricing) -> f64 {
    (pricing.net_cost.abs() * Decimal::from(CONTRACT_MULTIPLIER))
        .to_f64()
        .unwrap_or(0.0)
}

fn build_bpr_contexts(
    entry_context: BprPricingContext,
    sim: &HedgedSimulationOutput<CompositePricing>,
) -> Vec<BprPricingContext> {
    let mut contexts = Vec::with_capacity(2);
    let entry_ts = entry_context.ts;
    contexts.push(entry_context);

    if sim.exit_time != entry_ts {
        let fallback_spot = contexts[0].spot;
        let exit_spot = Decimal::try_from(sim.exit_spot).unwrap_or(fallback_spot);
        contexts.push(BprPricingContext {
            ts: sim.exit_time,
            spot: exit_spot,
            pricing: sim.exit_pricing.clone(),
        });
    }

    contexts
}

fn ensure_straddle_max_dte(
    event: &EarningsEvent,
    entry_date: NaiveDate,
    expiration: NaiveDate,
    max_short_dte: i32,
) -> Result<(), TradeGenerationError> {
    let dte = (expiration - entry_date).num_days() as i32;
    if dte <= max_short_dte {
        Ok(())
    } else {
        Err(TradeGenerationError {
            symbol: event.symbol.clone(),
            earnings_date: event.earnings_date,
            earnings_time: event.earnings_time,
            reason: "EXPIRATION_OUT_OF_RANGE".into(),
            details: Some(format!("DTE {} > max_short_dte {}", dte, max_short_dte)),
            phase: "filter".into(),
        })
    }
}

async fn maybe_attach_bpr_timeline<T, Pr, R>(
    result: &mut R,
    trade: &T,
    pricer: &Pr,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    timing: &TimingStrategy,
    hedge_position: Option<&HedgePosition>,
    pricing_contexts: Option<&[BprPricingContext]>,
    exec_config: &ExecutionConfig,
) where
    T: ExecutableTrade<Pricer = Pr> + CompositeTrade + Clone + Send + Sync,
    Pr: TradePricer<Trade = T, Pricing = CompositePricing>,
    R: HasBprTimeline,
{
    if matches!(exec_config.margin_config.mode, MarginMode::Off) {
        return;
    }

    match build_bpr_timeline(
        trade,
        pricer,
        options_repo,
        equity_repo,
        entry_time,
        exit_time,
        timing,
        hedge_position,
        pricing_contexts,
        &exec_config.margin_config,
    )
    .await
    {
        Ok(Some(timeline)) => {
            result.set_bpr_timeline(Some(timeline));
        }
        Ok(None) => {}
        Err(err) => {
            tracing::debug!(error = %err, "BPR timeline build failed");
        }
    }
}

// ============================================================================
// Concrete Strategy Implementations
// ============================================================================

/// Calendar Spread Strategy
pub struct CalendarSpreadStrategy {
    timing: TimingStrategy,
    option_type: finq_core::OptionType,
    rule_evaluator: RuleEvaluator,
}

impl CalendarSpreadStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);
        Self {
            timing,
            option_type: finq_core::OptionType::Call, // Default to calls
            rule_evaluator,
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<CalendarSpreadResult>> + Send + 'a>> {
        let option_type = self.option_type;
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 2. Select trade
            let trade = match selector
                .select_calendar_spread(&data.spot, &data.surface, option_type, criteria)
                .ok()
            {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            let pricer = CalendarSpreadPricer::new();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Simulate WITH HEDGING
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
        })
    }

}

/// Iron Butterfly Strategy
pub struct IronButterflyStrategy {
    timing: TimingStrategy,
    wing_width: Decimal,
    rule_evaluator: RuleEvaluator,
}

impl IronButterflyStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);
        Self {
            timing,
            wing_width: Decimal::from_f64_retain(config.wing_width)
                .unwrap_or_else(|| Decimal::new(5, 0)),
            rule_evaluator,
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<IronButterflyResult>> + Send + 'a>> {
        let wing_width = self.wing_width;
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 2. Select trade
            let trade = selector.select_iron_butterfly(
                &data.spot,
                &data.surface,
                wing_width,
                min_short_dte,
                max_short_dte,
            ).ok();
            let trade = match trade {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            let pricer = IronButterflyCompositePricer::new();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Simulate WITH HEDGING
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
        })
    }
}

/// Long Iron Butterfly Strategy (buy ATM straddle, sell wings - profits from volatility)
pub struct LongIronButterflyStrategy {
    timing: TimingStrategy,
    wing_width: Decimal,
    rule_evaluator: RuleEvaluator,
}

impl LongIronButterflyStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);
        Self {
            timing,
            wing_width: Decimal::from_f64_retain(config.wing_width)
                .unwrap_or_else(|| Decimal::new(5, 0)),
            rule_evaluator,
        }
    }

    pub fn with_wing_width(mut self, wing_width: Decimal) -> Self {
        self.wing_width = wing_width;
        self
    }
}

impl TradeStrategy<IronButterflyResult> for LongIronButterflyStrategy {
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<IronButterflyResult>> + Send + 'a>> {
        let wing_width = self.wing_width;
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 2. Select trade (LONG iron butterfly)
            let trade = selector.select_long_iron_butterfly(
                &data.spot,
                &data.surface,
                wing_width,
                min_short_dte,
                max_short_dte,
            ).ok();
            let trade = match trade {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            let pricer = LongIronButterflyCompositePricer::new();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Simulate WITH HEDGING
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
        })
    }
}

/// Long Straddle Strategy (pre-earnings)
pub struct LongStraddleStrategy {
    timing: TimingStrategy,
    /// Entry rules evaluator (checks IV slope, etc.)
    rule_evaluator: RuleEvaluator,
}

impl LongStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_straddle(
            config.timing,
            config.straddle_entry_days,
            config.straddle_exit_days,
        );
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);

        Self { timing, rule_evaluator }
    }
}

impl TradeStrategy<StraddleResult> for LongStraddleStrategy {
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            tracing::debug!(
                symbol = %event.symbol,
                "LongStraddleStrategy::execute_trade called"
            );

            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules (IV slope, etc.)
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Select trade (LONG straddle)
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            let trade = match selector
                .select_long_straddle(&data.spot, &data.surface, min_expiration)
                .ok()
            {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            if let Err(error) = ensure_straddle_max_dte(
                event,
                entry_date,
                trade.expiration(),
                max_short_dte,
            ) {
                return TradeExecutionOutcome::Dropped(error);
            }

            let pricer = CompositePricer::default();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 4. Simulate WITH HEDGING (integrated into execution loop)
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
        })
    }
}

/// Short Straddle Strategy (pre-earnings)
pub struct ShortStraddleStrategy {
    timing: TimingStrategy,
    /// Entry rules evaluator (checks IV slope, etc.)
    rule_evaluator: RuleEvaluator,
}

impl ShortStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_straddle(
            config.timing,
            config.straddle_entry_days,
            config.straddle_exit_days,
        );
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);

        Self { timing, rule_evaluator }
    }
}

impl TradeStrategy<StraddleResult> for ShortStraddleStrategy {
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            tracing::debug!(
                symbol = %event.symbol,
                "ShortStraddleStrategy::execute_trade called"
            );

            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules (IV slope, etc.)
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Select trade (SHORT straddle)
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            let trade = match selector
                .select_short_straddle(&data.spot, &data.surface, min_expiration)
                .ok()
            {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            if let Err(error) = ensure_straddle_max_dte(
                event,
                entry_date,
                trade.expiration(),
                max_short_dte,
            ) {
                return TradeExecutionOutcome::Dropped(error);
            }

            let pricer = ShortStraddlePricer::default();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Simulate WITH HEDGING (integrated into execution loop)
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
        })
    }
}

/// Backward compatibility alias
#[deprecated(since = "0.3.0", note = "Use LongStraddleStrategy or ShortStraddleStrategy")]
pub type StraddleStrategy = LongStraddleStrategy;

/// Post-Earnings Straddle Strategy
pub struct PostEarningsStraddleStrategy {
    timing: TimingStrategy,
    rule_evaluator: RuleEvaluator,
}

impl PostEarningsStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_post_earnings(
            config.timing,
            config.post_earnings_holding_days,
        );
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);
        Self { timing, rule_evaluator }
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<StraddleResult>> + Send + 'a>> {
        let min_short_dte = criteria.min_short_dte;
        let max_short_dte = criteria.max_short_dte;
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 2. Select trade (post-earnings uses LONG straddle)
            let entry_date = entry_time.date_naive();
            let min_expiration = (entry_date + chrono::Duration::days(min_short_dte as i64))
                .max(entry_date);
            let trade = match selector
                .select_long_straddle(&data.spot, &data.surface, min_expiration)
                .ok()
            {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            if let Err(error) = ensure_straddle_max_dte(
                event,
                entry_date,
                trade.expiration(),
                max_short_dte,
            ) {
                return TradeExecutionOutcome::Dropped(error);
            }

            let pricer = CompositePricer::default();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Simulate WITH HEDGING
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
        })
    }
}

/// Calendar Straddle Strategy
pub struct CalendarStraddleStrategy {
    timing: TimingStrategy,
    rule_evaluator: RuleEvaluator,
}

impl CalendarStraddleStrategy {
    pub fn new(config: &BacktestConfig) -> Self {
        let timing = TimingStrategy::for_earnings(config.timing);
        let rules_config = config.build_rules_config();
        let rule_evaluator = RuleEvaluator::new(rules_config);
        Self { timing, rule_evaluator }
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<CalendarStraddleResult>> + Send + 'a>> {
        let timing = self.timing.clone();
        let rule_evaluator = self.rule_evaluator.clone();
        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo, equity_repo, &event.symbol, entry_time, exit_time, exec_config,
            );

            // 1. Prepare market data
            let data = match simulator.prepare().await {
                Some(data) => data,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 2. Check market-level entry rules
            if let Err(error) = passes_market_rules(&rule_evaluator, event, &data) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 2. Select trade
            let trade = match selector
                .select_calendar_straddle(&data.spot, &data.surface, criteria)
                .ok()
            {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            let pricer = CalendarStraddleCompositePricer::new();
            let entry_pricing = match pricer.price_with_surface(
                &trade,
                &data.entry_chain,
                data.spot.to_f64(),
                entry_time,
                Some(&data.surface),
            ) {
                Ok(pricing) => pricing,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err.into()),
                    );
                }
            };

            let entry_price = entry_price_per_contract(&entry_pricing);
            if let Err(error) = passes_trade_rules(&rule_evaluator, event, entry_price) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 3. Simulate WITH HEDGING
            let bpr_entry_context = BprPricingContext {
                ts: entry_time,
                spot: data.spot.value,
                pricing: entry_pricing.clone(),
            };
            let entry_context = EntryPricingContext {
                pricing: entry_pricing,
                spot: data.spot.to_f64(),
                surface_time: Some(data.surface.as_of_time()),
            };
            let sim = match simulate_with_hedging_prepriced(
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                exec_config.hedge_config.as_ref(),
                &timing,
                entry_context,
            ).await {
                Ok(s) => s,
                Err(err) => {
                    return TradeExecutionOutcome::Executed(
                        trade.to_failed_result(&simulator.failed_output(), Some(event), err),
                    );
                }
            };

            // 4. Build result with hedge data
            let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
            let mut result = trade.to_result(
                sim.entry_pricing,
                sim.exit_pricing,
                &crate::execution::SimulationOutput::new(
                    sim.entry_time,
                    sim.exit_time,
                    sim.entry_spot,
                    sim.exit_spot,
                    sim.entry_surface_time,
                    sim.exit_surface_time,
                ),
                Some(event),
            );

            maybe_attach_bpr_timeline(
                &mut result,
                &trade,
                &pricer,
                options_repo,
                equity_repo,
                entry_time,
                exit_time,
                &timing,
                sim.hedge_position.as_ref(),
                Some(&bpr_contexts),
                exec_config,
            )
            .await;

            // 5. Apply hedge results if present
            if let Some(pos) = sim.hedge_position {
                let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
                let total_pnl = result.pnl + hedge_pnl - pos.total_cost;
                result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
            }

            TradeExecutionOutcome::Executed(result)
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
    LongIronButterfly(LongIronButterflyStrategy),
    LongStraddle(LongStraddleStrategy),
    ShortStraddle(ShortStraddleStrategy),
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
            SpreadType::LongIronButterfly => {
                StrategyDispatch::LongIronButterfly(LongIronButterflyStrategy::new(config))
            }
            SpreadType::Straddle => {
                StrategyDispatch::LongStraddle(LongStraddleStrategy::new(config))
            }
            SpreadType::ShortStraddle => {
                StrategyDispatch::ShortStraddle(ShortStraddleStrategy::new(config))
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
        assert!(matches!(strategy, StrategyDispatch::LongStraddle(_)));

        let strategy = StrategyDispatch::from_config(SpreadType::ShortStraddle, &config);
        assert!(matches!(strategy, StrategyDispatch::ShortStraddle(_)));
    }
}
