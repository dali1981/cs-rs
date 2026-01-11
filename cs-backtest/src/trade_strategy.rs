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
use std::sync::Arc;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use cs_domain::*;
use cs_domain::strike_selection::{StrikeSelector, ExpirationCriteria};
use crate::config::BacktestConfig;
use crate::bpr::{build_bpr_timeline, BprPricingContext, HasBprTimeline};
use crate::execution::{ExecutionConfig, ExecutableTrade, TradePricer};
use crate::execution::cost_helpers::{apply_costs_to_result, ToTradingContext};
use crate::timing_strategy::TimingStrategy;
use crate::backtest_use_case::{TradeResultMethods, TradeGenerationError};
use crate::backtest_use_case_helpers::{PreparedData, TradeSimulator};
use crate::hedging_simulator::{simulate_with_hedging_prepriced, EntryPricingContext, HedgedSimulationOutput};
use crate::composite_pricer::{
    CompositePricer, CompositePricing, CalendarSpreadPricer, IronButterflyCompositePricer,
    LongIronButterflyCompositePricer, CalendarStraddleCompositePricer, ShortStraddlePricer,
};
use crate::rules::RuleEvaluator;

/// Context for trade selection - contains all inputs needed for selection and validation
#[derive(Clone)]
pub struct SelectionContext<'a> {
    pub selector: &'a dyn StrikeSelector,
    pub data: &'a PreparedData,
    pub criteria: &'a ExpirationCriteria,
    pub event: &'a EarningsEvent,
    pub entry_time: DateTime<Utc>,
}

/// Core trait for trade execution strategies
///
/// Each strategy encapsulates:
/// - Timing logic (when to enter/exit)
/// - Trade selection logic (strategy-specific)
/// - Common execution flow (pricing, simulation, cost application)
///
/// # Associated Types
/// - `Trade`: The trade type (CalendarSpread, LongStraddle, etc.)
/// - `Pricer`: The pricer type for this trade
///
/// # Default Implementation
/// The `execute_trade` method has a default implementation that:
/// 1. Prepares market data
/// 2. Checks market-level rules
/// 3. Calls `select_trade` (strategy-specific)
/// 4. Calls `validate_selection` (optional, for DTE checks etc.)
/// 5. Prices and simulates the trade
/// 6. **Applies trading costs**
/// 7. Attaches BPR timeline and hedge results
///
/// Strategies only need to implement the required methods.
pub trait TradeStrategy<R>: Send + Sync
where
    R: TradeResultMethods + TradeResult + ApplyCosts + HasBprTimeline + Send,
{
    /// The trade type this strategy works with
    type Trade: ExecutableTrade<Pricing = CompositePricing, Result = R, Pricer = Self::Pricer>
        + CompositeTrade
        + Clone
        + Send
        + Sync;

    /// The pricer type for this trade
    type Pricer: TradePricer<Trade = Self::Trade, Pricing = CompositePricing> + Send + Sync + 'static;

    // ========================================================================
    // Required methods - each strategy must implement these
    // ========================================================================

    /// Get the timing strategy for this trade type
    fn timing(&self) -> &TimingStrategy;

    /// Get the rule evaluator for entry/trade rules
    fn rule_evaluator(&self) -> &RuleEvaluator;

    /// Create the pricer for this strategy
    fn create_pricer(&self) -> Self::Pricer;

    /// Select a trade using strategy-specific logic
    ///
    /// Returns `None` if no valid trade can be selected.
    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade>;

    // ========================================================================
    // Optional methods with defaults
    // ========================================================================

    /// Validate the selected trade (e.g., DTE range checks for straddles)
    ///
    /// Default: no validation (always passes)
    fn validate_selection(
        &self,
        _trade: &Self::Trade,
        _ctx: &SelectionContext<'_>,
    ) -> Result<(), TradeGenerationError> {
        Ok(())
    }

    /// Apply post-execution filters to the result
    ///
    /// Returns `true` if the result passes all filters, `false` if it should be dropped.
    fn apply_filter(&self, _result: &R, _min_iv_ratio: Option<f64>) -> bool {
        true
    }

    /// Create a filter rejection error for dropped trades
    fn create_filter_error(&self, result: &R, event: &EarningsEvent) -> Option<TradeGenerationError> {
        let _ = (result, event);
        None
    }

    /// Calculate lookahead days for earnings loading
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

    // ========================================================================
    // Default execute_trade implementation
    // ========================================================================

    /// Execute a single trade
    ///
    /// This default implementation handles the common execution flow:
    /// 1. Prepare market data
    /// 2. Check market-level rules
    /// 3. Select trade (calls `select_trade`)
    /// 4. Validate selection (calls `validate_selection`)
    /// 5. Price and simulate with hedging
    /// 6. **Apply trading costs**
    /// 7. Attach BPR timeline and hedge results
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
    ) -> Pin<Box<dyn Future<Output = TradeExecutionOutcome<R>> + Send + 'a>> {
        let timing = self.timing().clone();
        let rule_evaluator = self.rule_evaluator().clone();
        let pricer = self.create_pricer();

        Box::pin(async move {
            let simulator = TradeSimulator::new(
                options_repo,
                equity_repo,
                &event.symbol,
                entry_time,
                exit_time,
                exec_config,
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

            // 3. Build selection context
            let ctx = SelectionContext {
                selector,
                data: &data,
                criteria,
                event,
                entry_time,
            };

            // 4. Select trade (strategy-specific)
            let trade = match self.select_trade(&ctx) {
                Some(trade) => trade,
                None => return TradeExecutionOutcome::Skipped,
            };

            // 5. Validate selection (strategy-specific, e.g., DTE checks)
            if let Err(error) = self.validate_selection(&trade, &ctx) {
                return TradeExecutionOutcome::Dropped(error);
            }

            // 6. Execute common flow (pricing, simulation, costs, hedge results)
            execute_common(
                trade,
                pricer,
                &data,
                &simulator,
                &rule_evaluator,
                &timing,
                options_repo,
                equity_repo,
                exec_config,
                event,
                entry_time,
                exit_time,
            )
            .await
        })
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
// Common Execution Helper
// ============================================================================

/// Common execution flow for all strategies
///
/// This function handles the common execution logic after trade selection,
/// including pricing, simulation, cost application, and hedge result handling.
///
/// # Type Parameters
/// - `T`: The trade type (CalendarSpread, LongStraddle, etc.)
/// - `Pr`: The pricer type (must produce CompositePricing)
/// - `R`: The result type (CalendarSpreadResult, StraddleResult, etc.)
///
/// # Returns
/// `TradeExecutionOutcome<R>` - the trade result with costs applied
async fn execute_common<T, Pr, R>(
    trade: T,
    pricer: Pr,
    data: &PreparedData,
    simulator: &TradeSimulator<'_>,
    rule_evaluator: &RuleEvaluator,
    timing: &TimingStrategy,
    options_repo: &dyn OptionsDataRepository,
    equity_repo: &dyn EquityDataRepository,
    exec_config: &ExecutionConfig,
    event: &EarningsEvent,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
) -> TradeExecutionOutcome<R>
where
    T: ExecutableTrade<Pricer = Pr, Pricing = CompositePricing, Result = R> + CompositeTrade + Clone + Send + Sync,
    Pr: TradePricer<Trade = T, Pricing = CompositePricing> + Send + Sync,
    R: TradeResultMethods + TradeResult + ApplyCosts + HasBprTimeline + Send,
{
    // 1. Price entry
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

    // 2. Check trade-level rules
    let entry_price = entry_price_per_contract(&entry_pricing);
    if let Err(error) = passes_trade_rules(rule_evaluator, event, entry_price) {
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
        timing,
        entry_context,
    )
    .await
    {
        Ok(s) => s,
        Err(err) => {
            return TradeExecutionOutcome::Executed(
                trade.to_failed_result(&simulator.failed_output(), Some(event), err),
            );
        }
    };

    // 4. Build result
    let bpr_contexts = build_bpr_contexts(bpr_entry_context, &sim);
    let mut result = trade.to_result(
        sim.entry_pricing.clone(),
        sim.exit_pricing.clone(),
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

    // 5. APPLY TRADING COSTS (the fix!)
    if exec_config.has_trading_costs() {
        apply_costs_to_result(
            &mut result,
            &sim.entry_pricing,
            &sim.exit_pricing,
            &event.symbol,
            sim.entry_spot,
            sim.exit_spot,
            sim.entry_time,
            sim.exit_time,
            T::trade_type(),
            exec_config,
        );
    }

    // 6. Attach BPR timeline
    maybe_attach_bpr_timeline(
        &mut result,
        &trade,
        &pricer,
        options_repo,
        equity_repo,
        entry_time,
        exit_time,
        timing,
        sim.hedge_position.as_ref(),
        Some(&bpr_contexts),
        exec_config,
    )
    .await;

    // 7. Apply hedge results if present
    if let Some(pos) = sim.hedge_position {
        let hedge_pnl = pos.calculate_pnl(sim.exit_spot);
        let total_pnl = TradeResult::pnl(&result) + hedge_pnl - pos.total_cost;
        result.apply_hedge_results(pos, hedge_pnl, total_pnl, None);
    }

    TradeExecutionOutcome::Executed(result)
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
    type Trade = CalendarSpread;
    type Pricer = CalendarSpreadPricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        CalendarSpreadPricer::new()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        ctx.selector
            .select_calendar_spread(&ctx.data.spot, &ctx.data.surface, self.option_type, ctx.criteria)
            .ok()
    }
    // Uses default execute_trade implementation
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
    type Trade = IronButterfly;
    type Pricer = IronButterflyCompositePricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        IronButterflyCompositePricer::new()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        ctx.selector
            .select_iron_butterfly(
                &ctx.data.spot,
                &ctx.data.surface,
                self.wing_width,
                ctx.criteria.min_short_dte,
                ctx.criteria.max_short_dte,
            )
            .ok()
    }
    // Uses default execute_trade implementation
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
    type Trade = LongIronButterfly;
    type Pricer = LongIronButterflyCompositePricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        LongIronButterflyCompositePricer::new()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        ctx.selector
            .select_long_iron_butterfly(
                &ctx.data.spot,
                &ctx.data.surface,
                self.wing_width,
                ctx.criteria.min_short_dte,
                ctx.criteria.max_short_dte,
            )
            .ok()
    }
    // Uses default execute_trade implementation
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
    type Trade = LongStraddle;
    type Pricer = CompositePricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        CompositePricer::default()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        let entry_date = ctx.entry_time.date_naive();
        let min_expiration = (entry_date + chrono::Duration::days(ctx.criteria.min_short_dte as i64))
            .max(entry_date);
        ctx.selector
            .select_long_straddle(&ctx.data.spot, &ctx.data.surface, min_expiration)
            .ok()
    }

    fn validate_selection(
        &self,
        trade: &Self::Trade,
        ctx: &SelectionContext<'_>,
    ) -> Result<(), TradeGenerationError> {
        let entry_date = ctx.entry_time.date_naive();
        ensure_straddle_max_dte(
            ctx.event,
            entry_date,
            trade.expiration(),
            ctx.criteria.max_short_dte,
        )
    }
    // Uses default execute_trade implementation
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
    type Trade = ShortStraddle;
    type Pricer = ShortStraddlePricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        ShortStraddlePricer::default()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        let entry_date = ctx.entry_time.date_naive();
        let min_expiration = (entry_date + chrono::Duration::days(ctx.criteria.min_short_dte as i64))
            .max(entry_date);
        ctx.selector
            .select_short_straddle(&ctx.data.spot, &ctx.data.surface, min_expiration)
            .ok()
    }

    fn validate_selection(
        &self,
        trade: &Self::Trade,
        ctx: &SelectionContext<'_>,
    ) -> Result<(), TradeGenerationError> {
        let entry_date = ctx.entry_time.date_naive();
        ensure_straddle_max_dte(
            ctx.event,
            entry_date,
            trade.expiration(),
            ctx.criteria.max_short_dte,
        )
    }
    // Uses default execute_trade implementation
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
    type Trade = LongStraddle;
    type Pricer = CompositePricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        CompositePricer::default()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        let entry_date = ctx.entry_time.date_naive();
        let min_expiration = (entry_date + chrono::Duration::days(ctx.criteria.min_short_dte as i64))
            .max(entry_date);
        ctx.selector
            .select_long_straddle(&ctx.data.spot, &ctx.data.surface, min_expiration)
            .ok()
    }

    fn validate_selection(
        &self,
        trade: &Self::Trade,
        ctx: &SelectionContext<'_>,
    ) -> Result<(), TradeGenerationError> {
        let entry_date = ctx.entry_time.date_naive();
        ensure_straddle_max_dte(
            ctx.event,
            entry_date,
            trade.expiration(),
            ctx.criteria.max_short_dte,
        )
    }
    // Uses default execute_trade implementation
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
    type Trade = CalendarStraddle;
    type Pricer = CalendarStraddleCompositePricer;

    fn timing(&self) -> &TimingStrategy {
        &self.timing
    }

    fn rule_evaluator(&self) -> &RuleEvaluator {
        &self.rule_evaluator
    }

    fn create_pricer(&self) -> Self::Pricer {
        CalendarStraddleCompositePricer::new()
    }

    fn select_trade(&self, ctx: &SelectionContext<'_>) -> Option<Self::Trade> {
        ctx.selector
            .select_calendar_straddle(&ctx.data.spot, &ctx.data.surface, ctx.criteria)
            .ok()
    }
    // Uses default execute_trade implementation
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
