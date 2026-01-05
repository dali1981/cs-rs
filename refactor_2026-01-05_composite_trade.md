# Refactoring Plan: CompositeTrade Abstraction & Unified Executor

**Date**: 2026-01-05
**Status**: Draft
**Scope**: cs-domain, cs-backtest

## Problem Statement

### Current Issues

1. **Duplicate Execution Logic**: `TradeOrchestrator` has 4 nearly-identical methods for each trade type (lines 251-505)
2. **Missing Hedging in RollingExecutor**: `RollingExecutor` cannot hedge because it lacks `hedge_config` and `timing_strategy`
3. **Non-Generic TradeOrchestrator**: Despite having generic `apply_hedging<T: TradeResult>()`, the orchestrator hardcodes 4 trade types
4. **Boilerplate for New Strategies**: Adding a new option strategy requires ~500-800 lines across multiple files
5. **Duplicate Pricers**: 4 separate pricer structs that all do the same thing (price legs, sum with position signs)

### Files Affected

| File | Lines | Issue |
|------|-------|-------|
| `cs-backtest/src/rolling_executor.rs` | 281 | No hedging support |
| `cs-backtest/src/trade_orchestrator.rs` | 698 | 4 duplicate execute methods |
| `cs-backtest/src/straddle_pricer.rs` | 117 | Could be generic |
| `cs-backtest/src/iron_butterfly_pricer.rs` | 138 | Could be generic |
| `cs-backtest/src/calendar_straddle_pricer.rs` | 150 | Could be generic |
| `cs-backtest/src/spread_pricer.rs` | 400+ | Base pricer (keep) |

## Solution Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                     CompositeTrade Trait                            │
│  fn legs(&self) -> Vec<(&OptionLeg, LegPosition)>                   │
└─────────────────────────────────────────────────────────────────────┘
         │              │              │              │
         ▼              ▼              ▼              ▼
    ┌─────────┐   ┌──────────┐   ┌──────────┐   ┌────────────┐
    │Straddle │   │IronBfly  │   │CalSpread │   │CalStraddle │
    │ 2 legs  │   │ 4 legs   │   │ 2 legs   │   │  4 legs    │
    └─────────┘   └──────────┘   └──────────┘   └────────────┘
         │              │              │              │
         └──────────────┴──────────────┴──────────────┘
                                │
                                ▼
                  ┌─────────────────────────┐
                  │    CompositePricer      │
                  │  (generic over T)       │
                  └─────────────────────────┘
                                │
                                ▼
                  ┌─────────────────────────┐
                  │    TradeExecutor        │
                  │  - execute<T>()         │
                  │  - execute_rolling<T>() │
                  │  - apply_hedging()      │
                  └─────────────────────────┘
```

## Implementation Plan

### Phase 1: CompositeTrade Trait (cs-domain)

**File**: `cs-domain/src/trade/composite.rs`

```rust
//! Composite trade abstraction for multi-leg option strategies

use crate::entities::OptionLeg;

/// Position direction for a leg
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegPosition {
    Long,   // +1: bought, profit when price rises
    Short,  // -1: sold, profit when price falls
}

impl LegPosition {
    pub fn sign(&self) -> f64 {
        match self {
            LegPosition::Long => 1.0,
            LegPosition::Short => -1.0,
        }
    }

    pub fn sign_decimal(&self) -> rust_decimal::Decimal {
        match self {
            LegPosition::Long => rust_decimal::Decimal::ONE,
            LegPosition::Short => rust_decimal::Decimal::NEGATIVE_ONE,
        }
    }
}

/// Trait for multi-leg option strategies
///
/// Implementing this trait enables:
/// - Generic pricing (sum leg prices with position signs)
/// - Generic Greeks (sum leg Greeks with position signs)
/// - Generic hedging (net delta/gamma from legs)
pub trait CompositeTrade: Sized + Send + Sync {
    /// Returns all legs with their position (long/short)
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)>;

    /// Symbol (derived from first leg by default)
    fn symbol(&self) -> &str {
        self.legs().first().map(|(leg, _)| leg.symbol.as_str()).unwrap_or("")
    }

    /// Number of legs
    fn leg_count(&self) -> usize {
        self.legs().len()
    }
}
```

**Implementations** (in `cs-domain/src/entities.rs` or separate file):

```rust
impl CompositeTrade for Straddle {
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)> {
        vec![
            (&self.call_leg, LegPosition::Long),
            (&self.put_leg, LegPosition::Long),
        ]
    }
}

impl CompositeTrade for CalendarSpread {
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)> {
        vec![
            (&self.short_leg, LegPosition::Short),
            (&self.long_leg, LegPosition::Long),
        ]
    }
}

impl CompositeTrade for IronButterfly {
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)> {
        vec![
            (&self.short_call, LegPosition::Short),
            (&self.short_put, LegPosition::Short),
            (&self.long_call, LegPosition::Long),
            (&self.long_put, LegPosition::Long),
        ]
    }
}

impl CompositeTrade for CalendarStraddle {
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)> {
        vec![
            (&self.short_call, LegPosition::Short),
            (&self.short_put, LegPosition::Short),
            (&self.long_call, LegPosition::Long),
            (&self.long_put, LegPosition::Long),
        ]
    }
}
```

### Phase 2: Composite Pricing (cs-backtest)

**File**: `cs-backtest/src/composite_pricer.rs`

```rust
//! Generic pricer for any CompositeTrade

use chrono::{DateTime, Utc};
use polars::prelude::DataFrame;
use rust_decimal::Decimal;
use cs_analytics::IVSurface;
use cs_domain::trade::{CompositeTrade, LegPosition};

use crate::spread_pricer::{SpreadPricer, PricingError, LegPricing};

/// Pricing result for a composite trade
#[derive(Debug, Clone)]
pub struct CompositePricing {
    /// Individual leg pricings
    pub legs: Vec<(LegPricing, LegPosition)>,
    /// Net cost (positive = debit, negative = credit)
    pub net_cost: Decimal,
    /// Net Greeks
    pub net_delta: f64,
    pub net_gamma: f64,
    pub net_theta: f64,
    pub net_vega: f64,
    /// Average IV (simple average across legs)
    pub avg_iv: f64,
}

impl CompositePricing {
    pub fn from_legs(legs: Vec<(LegPricing, LegPosition)>) -> Self {
        let mut net_cost = Decimal::ZERO;
        let mut net_delta = 0.0;
        let mut net_gamma = 0.0;
        let mut net_theta = 0.0;
        let mut net_vega = 0.0;
        let mut iv_sum = 0.0;
        let mut iv_count = 0;

        for (pricing, position) in &legs {
            let sign = position.sign_decimal();
            let sign_f64 = position.sign();

            // Long = pay (positive), Short = receive (negative)
            net_cost += pricing.price * sign;

            if let Some(greeks) = &pricing.greeks {
                net_delta += greeks.delta * sign_f64;
                net_gamma += greeks.gamma * sign_f64;
                net_theta += greeks.theta * sign_f64;
                net_vega += greeks.vega * sign_f64;
            }

            if let Some(iv) = pricing.iv {
                iv_sum += iv;
                iv_count += 1;
            }
        }

        Self {
            legs,
            net_cost,
            net_delta,
            net_gamma,
            net_theta,
            net_vega,
            avg_iv: if iv_count > 0 { iv_sum / iv_count as f64 } else { 0.0 },
        }
    }

    /// Get pricing for a specific leg index
    pub fn leg(&self, index: usize) -> Option<&LegPricing> {
        self.legs.get(index).map(|(p, _)| p)
    }
}

/// Generic pricer for any composite trade
pub struct CompositePricer {
    inner: SpreadPricer,
}

impl CompositePricer {
    pub fn new(inner: SpreadPricer) -> Self {
        Self { inner }
    }

    /// Price any composite trade
    pub fn price<T: CompositeTrade>(
        &self,
        trade: &T,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CompositePricing, PricingError> {
        let mut leg_pricings = Vec::with_capacity(trade.leg_count());

        for (leg, position) in trade.legs() {
            let pricing = self.inner.price_leg(
                &leg.symbol,
                leg.strike.value(),
                leg.expiration,
                leg.option_type,
                chain_df,
                spot,
                timestamp,
                iv_surface,
            )?;

            leg_pricings.push((pricing, position));
        }

        Ok(CompositePricing::from_legs(leg_pricings))
    }
}
```

### Phase 3: Generic ExecutableTrade Implementation

**File**: `cs-backtest/src/execution/composite_impl.rs`

```rust
//! Generic ExecutableTrade implementation for CompositeTrade types

use cs_domain::trade::CompositeTrade;
use crate::composite_pricer::{CompositePricer, CompositePricing};
use super::traits::{TradePricer, ExecutableTrade};
use super::types::{ExecutionConfig, ExecutionContext, ExecutionError};

/// Wrapper to implement TradePricer for CompositePricer
impl<T: CompositeTrade> TradePricer for CompositePricerAdapter<T> {
    type Trade = T;
    type Pricing = CompositePricing;

    fn price_with_surface(
        &self,
        trade: &T,
        chain_df: &DataFrame,
        spot: f64,
        timestamp: DateTime<Utc>,
        iv_surface: Option<&IVSurface>,
    ) -> Result<CompositePricing, PricingError> {
        self.inner.price(trade, chain_df, spot, timestamp, iv_surface)
    }
}

/// Generic result type for composite trades
#[derive(Debug, Clone)]
pub struct CompositeResult {
    pub symbol: String,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,

    // Pricing
    pub entry_cost: Decimal,
    pub exit_value: Decimal,
    pub pnl: Decimal,
    pub pnl_pct: Decimal,

    // Greeks at entry
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // IV
    pub entry_iv: Option<f64>,
    pub exit_iv: Option<f64>,

    // Spot
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,

    // Hedging (populated by apply_hedging)
    pub hedge_position: Option<HedgePosition>,
    pub hedge_pnl: Option<Decimal>,
    pub total_pnl_with_hedge: Option<Decimal>,
}

impl cs_domain::TradeResult for CompositeResult {
    fn symbol(&self) -> &str { &self.symbol }
    fn pnl(&self) -> Decimal { self.pnl }
    fn entry_cost(&self) -> Decimal { self.entry_cost }
    fn exit_value(&self) -> Decimal { self.exit_value }
    fn success(&self) -> bool { self.success }
    fn entry_time(&self) -> DateTime<Utc> { self.entry_time }
    fn exit_time(&self) -> DateTime<Utc> { self.exit_time }
    fn spot_at_entry(&self) -> f64 { self.spot_at_entry }
    fn spot_at_exit(&self) -> f64 { self.spot_at_exit }
    fn net_delta(&self) -> Option<f64> { self.net_delta }
    fn net_gamma(&self) -> Option<f64> { self.net_gamma }
    fn hedge_pnl(&self) -> Option<Decimal> { self.hedge_pnl }
    fn total_pnl_with_hedge(&self) -> Option<Decimal> { self.total_pnl_with_hedge }

    fn apply_hedge_results(
        &mut self,
        position: HedgePosition,
        hedge_pnl: Decimal,
        total_pnl: Decimal,
        _attribution: Option<PositionAttribution>,
    ) {
        self.hedge_position = Some(position);
        self.hedge_pnl = Some(hedge_pnl);
        self.total_pnl_with_hedge = Some(total_pnl);
    }
}
```

### Phase 4: Unified TradeExecutor

**File**: `cs-backtest/src/trade_executor.rs` (NEW - replaces both RollingExecutor and TradeOrchestrator)

```rust
//! Unified trade executor with hedging support
//!
//! Replaces:
//! - RollingExecutor (adds hedging)
//! - TradeOrchestrator (makes generic)

use std::sync::Arc;
use chrono::{DateTime, NaiveDate, Utc};
use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    HedgeConfig, HedgeState,
    RollPolicy, RollingResult, RollPeriod, RollReason,
    MarketTime, TradingCalendar, TradeFactory,
    trade::{CompositeTrade, RollableTrade, TradeResult},
};
use crate::composite_pricer::CompositePricer;
use crate::execution::{ExecutableTrade, ExecutionConfig, execute_trade};
use crate::timing_strategy::TimingStrategy;

/// Unified executor for any composite trade
///
/// Supports:
/// - Single trade execution with hedging
/// - Rolling execution with hedging
/// - Any trade type implementing CompositeTrade + ExecutableTrade
pub struct TradeExecutor {
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    pricer: CompositePricer,
    trade_factory: Arc<dyn TradeFactory>,
    config: ExecutionConfig,

    // Hedging support (was missing from RollingExecutor)
    hedge_config: HedgeConfig,
    timing_strategy: Option<TimingStrategy>,

    // Rolling support
    roll_policy: Option<RollPolicy>,
}

impl TradeExecutor {
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        pricer: CompositePricer,
        trade_factory: Arc<dyn TradeFactory>,
        config: ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            pricer,
            trade_factory,
            config,
            hedge_config: HedgeConfig::default(),
            timing_strategy: None,
            roll_policy: None,
        }
    }

    // Builder methods
    pub fn with_hedging(mut self, config: HedgeConfig, timing: TimingStrategy) -> Self {
        self.hedge_config = config;
        self.timing_strategy = Some(timing);
        self
    }

    pub fn with_roll_policy(mut self, policy: RollPolicy) -> Self {
        self.roll_policy = Some(policy);
        self
    }

    /// Execute a single trade with optional hedging
    ///
    /// Works for ANY trade implementing CompositeTrade + ExecutableTrade
    pub async fn execute<T>(
        &self,
        trade: &T,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> T::Result
    where
        T: CompositeTrade + ExecutableTrade,
    {
        // 1. Execute trade using generic executor
        let mut result = execute_trade(
            trade,
            &self.pricer,
            self.options_repo.as_ref(),
            self.equity_repo.as_ref(),
            &self.config,
            event,
            entry_time,
            exit_time,
        ).await;

        // 2. Apply hedging if enabled (NOW WORKS FOR ALL TRADES!)
        if result.success() && self.hedge_config.is_enabled() {
            if let Some(ref timing) = self.timing_strategy {
                let rehedge_times = timing.rehedge_times(
                    entry_time,
                    exit_time,
                    &self.hedge_config.strategy
                );

                if let Err(e) = self.apply_hedging(&mut result, entry_time, exit_time, rehedge_times).await {
                    eprintln!("Hedging failed: {}", e);
                }
            }
        }

        result
    }

    /// Execute rolling strategy with hedging
    ///
    /// Works for ANY trade implementing CompositeTrade + RollableTrade + ExecutableTrade
    pub async fn execute_rolling<T>(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        entry_time: MarketTime,
        exit_time: MarketTime,
    ) -> RollingResult
    where
        T: CompositeTrade + RollableTrade + ExecutableTrade,
    {
        let roll_policy = self.roll_policy.clone()
            .unwrap_or(RollPolicy::Weekly);

        let mut rolls = Vec::new();
        let mut current_date = start_date;

        if !TradingCalendar::is_trading_day(current_date) {
            current_date = TradingCalendar::next_trading_day(current_date);
        }

        while current_date < end_date {
            let entry_dt = self.to_datetime(current_date, entry_time);
            let min_expiration = current_date + chrono::Duration::days(1);

            // Create trade using trait method
            let trade = match T::create(
                self.trade_factory.as_ref(),
                symbol,
                entry_dt,
                min_expiration,
            ).await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to create trade at {}: {}", current_date, e);
                    current_date = TradingCalendar::next_trading_day(current_date);
                    continue;
                }
            };

            let (exit_date, roll_reason) = self.determine_exit_date(
                current_date,
                end_date,
                trade.expiration(),
                &roll_policy,
            );

            let exit_dt = self.to_datetime(exit_date, exit_time);

            // Create dummy earnings event
            let event = EarningsEvent::new(
                symbol.to_string(),
                exit_dt.date_naive(),
                EarningsTime::AfterMarketClose,
            );

            // Execute WITH HEDGING (the key fix!)
            let result = self.execute(&trade, &event, entry_dt, exit_dt).await;

            // Convert to roll period (now includes hedge data!)
            let roll_period = self.to_roll_period(&trade, result, roll_reason);
            rolls.push(roll_period);

            current_date = TradingCalendar::next_trading_day(exit_date);
        }

        let trade_type = std::any::type_name::<T>()
            .split("::")
            .last()
            .unwrap_or("unknown")
            .to_lowercase();

        RollingResult::from_rolls(
            symbol.to_string(),
            start_date,
            end_date,
            roll_policy.description(),
            trade_type,
            rolls,
        )
    }

    /// Apply hedging to any trade result (trade-agnostic)
    ///
    /// Moved from TradeOrchestrator - now shared by all execution paths
    async fn apply_hedging<T: TradeResult>(
        &self,
        result: &mut T,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> Result<(), String> {
        let net_delta = result.net_delta().unwrap_or(0.0);
        let net_gamma = result.net_gamma().unwrap_or(0.0);
        let entry_spot = result.spot_at_entry();
        let exit_spot = result.spot_at_exit();
        let symbol = result.symbol();

        let mut hedge_state = HedgeState::new(
            self.hedge_config.clone(),
            net_delta,
            net_gamma,
            entry_spot,
        );

        for rehedge_time in rehedge_times {
            if hedge_state.at_max_rehedges() {
                break;
            }

            let spot = self.equity_repo
                .get_spot_price(symbol, rehedge_time)
                .await
                .map_err(|e| format!("Failed to get spot at {}: {}", rehedge_time, e))?;

            hedge_state.update(rehedge_time, spot.to_f64());
        }

        let hedge_position = hedge_state.finalize(exit_spot);

        if hedge_position.rehedge_count() > 0 {
            let hedge_pnl = hedge_position.calculate_pnl(exit_spot);
            let total_pnl = result.pnl() + hedge_pnl - hedge_position.total_cost;

            result.apply_hedge_results(hedge_position, hedge_pnl, total_pnl, None);
        }

        Ok(())
    }

    fn determine_exit_date(
        &self,
        entry_date: NaiveDate,
        campaign_end: NaiveDate,
        expiration: NaiveDate,
        roll_policy: &RollPolicy,
    ) -> (NaiveDate, RollReason) {
        if campaign_end <= entry_date {
            return (entry_date, RollReason::EndOfCampaign);
        }

        let next_roll = roll_policy.next_roll_date(entry_date)
            .unwrap_or(campaign_end);

        let exit_date = next_roll.min(expiration).min(campaign_end);

        let reason = if exit_date >= campaign_end {
            RollReason::EndOfCampaign
        } else if exit_date >= expiration {
            RollReason::Expiry
        } else {
            RollReason::Scheduled
        };

        (exit_date, reason)
    }

    fn to_roll_period<T>(
        &self,
        trade: &T,
        result: T::Result,
        roll_reason: RollReason,
    ) -> RollPeriod
    where
        T: CompositeTrade + RollableTrade,
        T::Result: TradeResult,
    {
        RollPeriod {
            entry_date: result.entry_time().date_naive(),
            exit_date: result.exit_time().date_naive(),
            strike: trade.strike(),
            expiration: trade.expiration(),

            entry_debit: result.entry_cost(),
            exit_credit: result.exit_value(),
            pnl: result.pnl(),

            spot_at_entry: result.spot_at_entry(),
            spot_at_exit: result.spot_at_exit(),
            spot_move_pct: ((result.spot_at_exit() - result.spot_at_entry())
                / result.spot_at_entry() * 100.0),

            // Greeks from result (now populated!)
            net_delta: result.net_delta(),
            net_gamma: result.net_gamma(),
            net_theta: None, // Add to TradeResult if needed
            net_vega: None,

            // IV from result
            iv_entry: None, // Add to TradeResult if needed
            iv_exit: None,
            iv_change: None,

            // P&L attribution
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,

            // Hedging (NOW POPULATED!)
            hedge_pnl: result.hedge_pnl(),
            hedge_count: result.hedge_position()
                .map(|p| p.rehedge_count())
                .unwrap_or(0),
            transaction_cost: result.hedge_position()
                .map(|p| p.total_cost)
                .unwrap_or(Decimal::ZERO),

            roll_reason,
            position_attribution: None,
        }
    }

    fn to_datetime(&self, date: NaiveDate, time: MarketTime) -> DateTime<Utc> {
        use chrono::NaiveTime;
        use cs_domain::datetime::eastern_to_utc;

        let naive_time = NaiveTime::from_hms_opt(time.hour as u32, time.minute as u32, 0)
            .unwrap_or_else(|| NaiveTime::from_hms_opt(15, 45, 0).unwrap());

        eastern_to_utc(date, naive_time)
    }
}
```

### Phase 5: Migration Path

#### Step 1: Add CompositeTrade trait (non-breaking)

1. Create `cs-domain/src/trade/composite.rs`
2. Implement `CompositeTrade` for all 4 trade types
3. Export from `cs-domain/src/trade/mod.rs`
4. **No existing code changes**

#### Step 2: Add CompositePricer (non-breaking)

1. Create `cs-backtest/src/composite_pricer.rs`
2. Implement generic pricing
3. Add tests comparing output to existing pricers
4. **Existing pricers still work**

#### Step 3: Create TradeExecutor (non-breaking)

1. Create `cs-backtest/src/trade_executor.rs`
2. Implement unified execution with hedging
3. Add tests for rolling + hedging
4. **RollingExecutor and TradeOrchestrator still work**

#### Step 4: Migrate callers (breaking)

1. Update CLI commands to use `TradeExecutor`
2. Update tests
3. Mark `RollingExecutor` as deprecated
4. Mark `TradeOrchestrator` as deprecated

#### Step 5: Remove legacy code

1. Delete `RollingExecutor`
2. Delete `TradeOrchestrator`
3. Delete type-specific pricers (keep `SpreadPricer` as base)
4. Delete type-specific `ExecutableTrade` impls

## Files to Create

| File | Purpose |
|------|---------|
| `cs-domain/src/trade/composite.rs` | CompositeTrade trait + LegPosition |
| `cs-backtest/src/composite_pricer.rs` | Generic CompositePricer |
| `cs-backtest/src/execution/composite_impl.rs` | Generic ExecutableTrade |
| `cs-backtest/src/trade_executor.rs` | Unified TradeExecutor |

## Files to Delete (after migration)

| File | Replaced By |
|------|-------------|
| `cs-backtest/src/rolling_executor.rs` | `trade_executor.rs` |
| `cs-backtest/src/trade_orchestrator.rs` | `trade_executor.rs` |
| `cs-backtest/src/straddle_pricer.rs` | `composite_pricer.rs` |
| `cs-backtest/src/iron_butterfly_pricer.rs` | `composite_pricer.rs` |
| `cs-backtest/src/calendar_straddle_pricer.rs` | `composite_pricer.rs` |
| `cs-backtest/src/execution/straddle_impl.rs` | `composite_impl.rs` |
| `cs-backtest/src/execution/iron_butterfly_impl.rs` | `composite_impl.rs` |
| `cs-backtest/src/execution/calendar_spread_impl.rs` | `composite_impl.rs` |
| `cs-backtest/src/execution/calendar_straddle_impl.rs` | `composite_impl.rs` |

## Impact Summary

| Metric | Before | After | Reduction |
|--------|--------|-------|-----------|
| Pricer files | 4 | 1 | 75% |
| Executor files | 2 | 1 | 50% |
| Execution impl files | 4 | 1 | 75% |
| Lines (estimated) | ~2500 | ~800 | 68% |
| Adding new strategy | ~500 lines | ~50 lines | 90% |

## Testing Strategy

1. **Unit tests for CompositeTrade**: Verify leg enumeration for all types
2. **Unit tests for CompositePricer**: Compare output to existing pricers
3. **Integration tests for TradeExecutor**:
   - Single trade execution matches TradeOrchestrator
   - Rolling execution matches RollingExecutor
   - Rolling + hedging produces valid results
4. **Property tests**: Net Greeks = sum of leg Greeks

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Subtle pricing differences | Compare with existing pricers in tests |
| Breaking existing callers | Phased migration with deprecation warnings |
| Performance regression | Benchmark before/after |
| Missing edge cases | Comprehensive test coverage |

## Open Questions

1. Should `CompositePricing` store individual leg pricings or just aggregates?
2. Should we keep type-specific result types (`StraddleResult`, etc.) or use generic `CompositeResult`?
3. How to handle trade-specific validation (e.g., IronButterfly wing width)?

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-01-05 | Use trait-based approach | Enables generic code without runtime dispatch |
| 2026-01-05 | Keep `SpreadPricer` as base | Already handles single-leg pricing well |
| 2026-01-05 | Merge RollingExecutor + TradeOrchestrator | Both do execution, only differ in rolling vs hedging |
