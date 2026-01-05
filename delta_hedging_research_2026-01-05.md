# Delta Hedging Feature - Research & Implementation Plan

**Date:** 2026-01-05
**Status:** Research Complete, Ready for Implementation

---

## Executive Summary

This document outlines the research findings and implementation plan for adding delta hedging functionality to the options backtesting system. The existing infrastructure is well-suited for this feature - Greeks are already computed, stored, and aggregatable. The main work involves orchestrating periodic rehedges and tracking cumulative hedge positions.

---

## Part 1: Research Findings

### 1.1 Current Greeks Infrastructure

**Status: Excellent - Ready to Use**

#### Greeks Struct (`cs-analytics/src/greeks.rs`)
```rust
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
    pub rho: f64,
}
```

Features:
- Immutable value object
- Arithmetic operations: `Add`, `Sub`, `Mul`, `Neg`
- Helper methods: `position()`, `spread()`, `at_expiry()`
- Can aggregate Greeks across legs

#### Black-Scholes Implementation (`cs-analytics/src/black_scholes.rs`)
- `bs_greeks(spot, strike, time_to_expiry, vol, rate, option_type)` - All Greeks in one pass
- `bs_delta(...)` - Isolated delta calculation
- `bs_implied_volatility(...)` - IV solving via Brent's method
- Risk-free rate configurable (default 5%)

#### Greeks Storage in Trade Results
| Result Type | Delta Fields |
|-------------|--------------|
| `StraddleResult` | `net_delta`, `net_gamma`, `net_theta`, `net_vega` |
| `CalendarSpreadResult` | `short_delta`, `long_delta`, `short_gamma`, etc. |
| `IronButterflyResult` | `net_delta`, `net_gamma`, `net_theta`, `net_vega` |
| `CalendarStraddleResult` | Per-leg Greeks |

**Conclusion:** Greeks infrastructure is complete. No modifications needed for delta hedging calculations.

---

### 1.2 Position Tracking During Backtests

**Status: Good - Needs Extension for Hedging**

#### Current Execution Flow
```
BacktestUseCase
    └── process_event_unified(event, selector, structure, timing)
            └── UnifiedExecutor.execute_with_selection()
                    ├── StraddleExecutor.execute_trade()
                    ├── TradeExecutor.execute_trade() [Calendar]
                    ├── IronButterflyExecutor.execute_trade()
                    └── CalendarStraddleExecutor.execute_trade()
```

#### Current Position Representation
Each executor tracks:
- **Entry prices**: Premium paid/received at entry
- **Exit prices**: Premium received/paid at exit
- **Spot prices**: `spot_at_entry`, `spot_at_exit`
- **Greeks at entry**: Computed once, stored in result
- **P&L attribution**: `delta_pnl`, `gamma_pnl`, `theta_pnl`, `vega_pnl`

#### Current Limitation
- Only two snapshots: entry and exit
- No intermediate position tracking
- No dynamic rehedging capability
- Greeks not recomputed during holding period

**Conclusion:** Need to add position state tracking that persists across rehedge events.

---

### 1.3 Pricing Models

**Status: Excellent - Ready to Use**

#### Pricing Fallback Chain (`spread_pricer.rs`)
1. **Exact market data match** - Use if available
2. **Put-call parity conversion** - If only one side available
3. **IV surface interpolation** - Sticky strike/moneyness/delta models
4. **Black-Scholes model** - Last resort with interpolated IV

#### IV Surface Models (`cs-analytics/src/vol_slice.rs`)
- **Linear interpolation** (M1): Simple delta-space interpolation
- **SVI fitting** (M2): Parametric smile for extrapolation

#### Pricing Model Enum
```rust
pub enum PricingModel {
    StickyStrike,      // Default
    StickyMoneyness,
    StickyDelta,
}
```

**Conclusion:** Pricing infrastructure is robust. Can reuse for rehedge pricing.

---

### 1.4 Timing System

**Status: Good - Needs Extension**

#### Current Timing Strategies (`cs-backtest/src/timing_strategy.rs`)
```rust
pub enum TimingStrategy {
    Earnings(EarningsTradeTiming),      // Enter on/near earnings
    Straddle(StraddleTradeTiming),      // Enter N days before
    PostEarnings(PostEarningsStraddleTiming),  // Enter after earnings
}
```

#### Timing Methods
- `entry_datetime(event)` - Single entry timestamp
- `exit_datetime(event)` - Single exit timestamp
- `entry_date(event)` - Entry date (NaiveDate)
- `lookahead_days()` - For event loading

#### Current Limitation
- Returns single entry and single exit time
- No support for intermediate timestamps
- No periodic interval computation

**Conclusion:** Need to extend with `rehedge_times()` method.

---

### 1.5 Data Repositories

**Status: Good - May Need Extension**

#### EquityDataRepository (`cs-domain/src/repositories.rs`)
```rust
pub trait EquityDataRepository {
    async fn get_spot_price(&self, symbol: &str, timestamp: DateTime<Utc>)
        -> Result<SpotPrice, RepositoryError>;
}
```

#### OptionsDataRepository
```rust
pub trait OptionsDataRepository {
    async fn get_option_bars_at_time(&self, symbol: &str, timestamp: DateTime<Utc>)
        -> Result<DataFrame, RepositoryError>;

    async fn get_option_bars_at_or_after_time(&self, symbol: &str, timestamp: DateTime<Utc>, max_minutes: i64)
        -> Result<(DataFrame, DateTime<Utc>), RepositoryError>;
}
```

**Conclusion:** Current repos support point-in-time queries. May benefit from batch loading for efficiency, but not required.

---

## Part 2: Design Decisions

### 2.1 Hedging Strategy Options

| Strategy | Description | Trigger | Best For |
|----------|-------------|---------|----------|
| **Time-Based** | Rehedge at fixed intervals | Every N hours/days | Regular gamma scalping |
| **Delta-Threshold** | Rehedge when delta exceeds threshold | \|Δ\| > 0.10 | Cost-efficient hedging |
| **Gamma-Based** | Rehedge based on gamma exposure | \|Γ × ΔS²\| > threshold | Risk-based hedging |
| **Hybrid** | Time-based with delta override | Both conditions | Production-style |

**Recommendation:** Implement Delta-Threshold first (most common), then Time-Based.

### 2.2 Hedge Execution Model

#### Option A: Perfect Hedge (Simpler)
- Assume we can always hedge at exact spot price
- No slippage, no bid-ask spread
- Good for initial implementation

#### Option B: Realistic Hedge (Complex)
- Include bid-ask spread on stock
- Model execution slippage
- Transaction costs per trade

**Recommendation:** Start with Option A, add costs as configuration option later.

### 2.3 Greeks Recomputation

#### Option A: Full Recompute (Accurate)
- At each rehedge time:
  - Load option chain
  - Build IV surface
  - Compute Greeks via Black-Scholes
- Most accurate, highest data/compute cost

#### Option B: Interpolate (Fast)
- Compute Greeks at entry
- At rehedge: estimate delta change from spot move and gamma
- `delta_new ≈ delta_old + gamma × (spot_new - spot_old)`
- Faster, less accurate for large moves

**Recommendation:** Option A for backtesting (accuracy matters), Option B available for live trading.

### 2.4 Position Delta Calculation

For a straddle (long call + long put):
```
Position Delta = call_delta + put_delta
               ≈ 0.50 + (-0.50) = 0.00  (at-the-money)
```

To hedge to delta-neutral:
```
Shares to Trade = -Position Delta × Contract Multiplier
                = -0.05 × 100 = -5 shares (sell 5 shares)
```

After hedge:
```
Total Delta = Option Delta + Stock Delta
            = 0.05 + (-0.05) = 0.00
```

---

## Part 3: Implementation Plan

### Phase 1: Domain Layer Foundation

**Duration:** ~2 hours

#### 1.1 Create Hedging Value Objects

**File:** `cs-domain/src/hedging.rs` (NEW)

```rust
use chrono::{DateTime, Utc, Duration};
use rust_decimal::Decimal;

/// A single hedge transaction
#[derive(Debug, Clone)]
pub struct HedgeAction {
    pub timestamp: DateTime<Utc>,
    pub shares: i32,              // Positive = buy, negative = sell
    pub spot_price: f64,
    pub delta_before: f64,        // Position delta before hedge
    pub delta_after: f64,         // Position delta after hedge
    pub cost: Decimal,            // Transaction cost (optional)
}

/// Cumulative hedge position state
#[derive(Debug, Clone, Default)]
pub struct HedgePosition {
    pub cumulative_shares: i32,   // Net shares held
    pub hedges: Vec<HedgeAction>, // All hedge transactions
    pub realized_pnl: Decimal,    // P&L from closed hedge portions
    pub unrealized_pnl: Decimal,  // P&L from open hedge position
    pub total_cost: Decimal,      // Sum of transaction costs
}

impl HedgePosition {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new hedge action
    pub fn add_hedge(&mut self, action: HedgeAction) {
        self.cumulative_shares += action.shares;
        self.total_cost += action.cost;
        self.hedges.push(action);
    }

    /// Calculate hedge P&L at exit
    pub fn calculate_pnl(&self, exit_spot: f64) -> Decimal {
        // Sum of (shares × (exit_spot - hedge_spot)) for each hedge
        self.hedges.iter().map(|h| {
            let pnl_per_share = exit_spot - h.spot_price;
            Decimal::try_from(h.shares as f64 * pnl_per_share).unwrap_or_default()
        }).sum()
    }

    /// Number of rehedges performed
    pub fn rehedge_count(&self) -> usize {
        self.hedges.len()
    }

    /// Average hedge price (for reporting)
    pub fn average_hedge_price(&self) -> Option<f64> {
        if self.hedges.is_empty() {
            return None;
        }
        let total_value: f64 = self.hedges.iter()
            .map(|h| h.shares as f64 * h.spot_price)
            .sum();
        let total_shares: i32 = self.hedges.iter()
            .map(|h| h.shares)
            .sum();
        if total_shares == 0 {
            None
        } else {
            Some(total_value / total_shares as f64)
        }
    }
}

/// Hedging strategy configuration
#[derive(Debug, Clone)]
pub enum HedgeStrategy {
    /// Rehedge at fixed time intervals
    TimeBased {
        interval: Duration,
    },
    /// Rehedge when absolute delta exceeds threshold
    DeltaThreshold {
        threshold: f64,  // e.g., 0.10 = rehedge when |delta| > 0.10
    },
    /// Rehedge based on dollar gamma exposure
    GammaDollar {
        threshold: f64,  // Rehedge when |gamma × spot² × 0.01| > threshold
    },
    /// No hedging (baseline)
    None,
}

impl Default for HedgeStrategy {
    fn default() -> Self {
        HedgeStrategy::None
    }
}

/// Configuration for delta hedging
#[derive(Debug, Clone)]
pub struct HedgeConfig {
    pub strategy: HedgeStrategy,
    pub max_rehedges: Option<usize>,      // Limit number of rehedges
    pub min_hedge_size: i32,              // Minimum shares to trade
    pub transaction_cost_per_share: Decimal,  // Cost per share traded
    pub contract_multiplier: i32,         // Usually 100 for options
}

impl Default for HedgeConfig {
    fn default() -> Self {
        Self {
            strategy: HedgeStrategy::None,
            max_rehedges: None,
            min_hedge_size: 1,
            transaction_cost_per_share: Decimal::ZERO,
            contract_multiplier: 100,
        }
    }
}

impl HedgeConfig {
    /// Check if hedging is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self.strategy, HedgeStrategy::None)
    }

    /// Determine if rehedge is needed based on strategy
    pub fn should_rehedge(&self, position_delta: f64, spot: f64, gamma: f64) -> bool {
        match &self.strategy {
            HedgeStrategy::None => false,
            HedgeStrategy::TimeBased { .. } => true,  // Always rehedge at scheduled times
            HedgeStrategy::DeltaThreshold { threshold } => {
                position_delta.abs() > *threshold
            }
            HedgeStrategy::GammaDollar { threshold } => {
                // Dollar gamma = gamma × spot² × 0.01 (for 1% move)
                let dollar_gamma = gamma.abs() * spot * spot * 0.01;
                dollar_gamma > *threshold
            }
        }
    }

    /// Calculate shares needed to delta-neutralize
    pub fn shares_to_hedge(&self, position_delta: f64) -> i32 {
        // Shares = -delta × multiplier
        let raw_shares = (-position_delta * self.contract_multiplier as f64).round() as i32;

        // Apply minimum size filter
        if raw_shares.abs() < self.min_hedge_size {
            0
        } else {
            raw_shares
        }
    }
}
```

#### 1.2 Add Hedging Fields to Result Structs

**File:** `cs-domain/src/entities.rs` (MODIFY)

Add to `StraddleResult`:
```rust
pub struct StraddleResult {
    // ... existing fields ...

    // Hedging fields (optional)
    pub hedge_position: Option<HedgePosition>,
    pub hedge_pnl: Option<Decimal>,
    pub total_pnl_with_hedge: Option<Decimal>,
}
```

---

### Phase 2: Timing System Extension

**Duration:** ~2 hours

#### 2.1 Add Rehedge Schedule to Timing Strategy

**File:** `cs-backtest/src/timing_strategy.rs` (MODIFY)

```rust
impl TimingStrategy {
    // ... existing methods ...

    /// Compute rehedge timestamps between entry and exit
    pub fn rehedge_times(
        &self,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        strategy: &HedgeStrategy,
    ) -> Vec<DateTime<Utc>> {
        match strategy {
            HedgeStrategy::None => vec![],
            HedgeStrategy::TimeBased { interval } => {
                let mut times = Vec::new();
                let mut current = entry_time + *interval;
                while current < exit_time {
                    times.push(current);
                    current = current + *interval;
                }
                times
            }
            HedgeStrategy::DeltaThreshold { .. } | HedgeStrategy::GammaDollar { .. } => {
                // For threshold-based strategies, check at regular intervals
                // but only actually hedge if threshold exceeded
                self.generate_check_times(entry_time, exit_time)
            }
        }
    }

    /// Generate times to check delta (for threshold strategies)
    fn generate_check_times(
        &self,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> Vec<DateTime<Utc>> {
        // Check every hour during market hours
        let check_interval = chrono::Duration::hours(1);
        let mut times = Vec::new();
        let mut current = entry_time + check_interval;
        while current < exit_time {
            // Only include times during market hours (14:30 - 21:00 UTC = 9:30 - 16:00 ET)
            let hour = current.hour();
            if hour >= 14 && hour < 21 {
                times.push(current);
            }
            current = current + check_interval;
        }
        times
    }
}
```

---

### Phase 3: Hedging Executor

**Duration:** ~4 hours

#### 3.1 Create Hedging Executor Wrapper

**File:** `cs-backtest/src/hedging_executor.rs` (NEW)

```rust
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EarningsEvent, StraddleResult, HedgeConfig, HedgePosition, HedgeAction,
    EquityDataRepository, OptionsDataRepository,
};
use cs_analytics::{bs_greeks, Greeks};

use crate::straddle_executor::StraddleExecutor;
use crate::straddle_pricer::StraddlePricing;
use crate::iv_surface_builder::build_iv_surface_minute_aligned;

/// Executor wrapper that adds delta hedging to any position
pub struct HedgingExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    inner_executor: StraddleExecutor<O, E>,
    equity_repo: Arc<E>,
    options_repo: Arc<O>,
    hedge_config: HedgeConfig,
}

impl<O, E> HedgingExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub fn new(
        inner_executor: StraddleExecutor<O, E>,
        equity_repo: Arc<E>,
        options_repo: Arc<O>,
        hedge_config: HedgeConfig,
    ) -> Self {
        Self {
            inner_executor,
            equity_repo,
            options_repo,
            hedge_config,
        }
    }

    /// Execute trade with delta hedging
    pub async fn execute_with_hedging(
        &self,
        straddle: &Straddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        rehedge_times: Vec<DateTime<Utc>>,
    ) -> StraddleResult {
        // 1. Execute base trade (entry pricing)
        let base_result = self.inner_executor
            .execute_trade(straddle, event, entry_time, exit_time)
            .await;

        if !base_result.success || !self.hedge_config.is_enabled() {
            return base_result;
        }

        // 2. Initialize hedge position
        let mut hedge_position = HedgePosition::new();
        let mut current_delta = base_result.net_delta.unwrap_or(0.0);

        // 3. Process each rehedge time
        for rehedge_time in rehedge_times {
            // Skip if we've hit max rehedges
            if let Some(max) = self.hedge_config.max_rehedges {
                if hedge_position.rehedge_count() >= max {
                    break;
                }
            }

            // Get spot price at rehedge time
            let spot = match self.equity_repo
                .get_spot_price(straddle.symbol(), rehedge_time)
                .await
            {
                Ok(s) => s.to_f64(),
                Err(_) => continue,  // Skip if no data
            };

            // Recompute Greeks (simplified: use gamma approximation)
            let gamma = base_result.net_gamma.unwrap_or(0.0);
            let spot_change = spot - base_result.spot_at_entry;
            let new_delta = current_delta + gamma * spot_change;

            // Check if rehedge needed
            if !self.hedge_config.should_rehedge(new_delta, spot, gamma) {
                current_delta = new_delta;
                continue;
            }

            // Calculate hedge
            let shares = self.hedge_config.shares_to_hedge(new_delta);
            if shares == 0 {
                continue;
            }

            // Record hedge action
            let cost = self.hedge_config.transaction_cost_per_share
                * Decimal::from(shares.abs());

            let action = HedgeAction {
                timestamp: rehedge_time,
                shares,
                spot_price: spot,
                delta_before: new_delta,
                delta_after: new_delta + (shares as f64 / self.hedge_config.contract_multiplier as f64),
                cost,
            };

            hedge_position.add_hedge(action);

            // Update current delta (now includes stock position)
            current_delta = new_delta + (shares as f64 / self.hedge_config.contract_multiplier as f64);
        }

        // 4. Calculate hedge P&L at exit
        let hedge_pnl = hedge_position.calculate_pnl(base_result.spot_at_exit);
        let total_pnl = base_result.pnl + hedge_pnl - hedge_position.total_cost;

        // 5. Return enhanced result
        StraddleResult {
            hedge_position: Some(hedge_position),
            hedge_pnl: Some(hedge_pnl),
            total_pnl_with_hedge: Some(total_pnl),
            ..base_result
        }
    }
}
```

---

### Phase 4: CLI Integration

**Duration:** ~2 hours

#### 4.1 Add Hedging CLI Arguments

**File:** `cs-cli/src/cli_args.rs` (MODIFY)

```rust
#[derive(Parser)]
pub struct BacktestArgs {
    // ... existing args ...

    /// Enable delta hedging
    #[arg(long, default_value = "false")]
    pub hedge: bool,

    /// Hedging strategy: "time", "delta", "gamma"
    #[arg(long, default_value = "delta")]
    pub hedge_strategy: String,

    /// For time-based: rehedge interval in hours
    #[arg(long, default_value = "24")]
    pub hedge_interval_hours: u64,

    /// For delta-based: threshold to trigger rehedge
    #[arg(long, default_value = "0.10")]
    pub delta_threshold: f64,

    /// Maximum number of rehedges per trade
    #[arg(long)]
    pub max_rehedges: Option<usize>,

    /// Transaction cost per share
    #[arg(long, default_value = "0.01")]
    pub hedge_cost_per_share: f64,
}
```

#### 4.2 Parse Hedge Config

**File:** `cs-cli/src/main.rs` (MODIFY)

```rust
fn build_hedge_config(args: &BacktestArgs) -> HedgeConfig {
    if !args.hedge {
        return HedgeConfig::default();
    }

    let strategy = match args.hedge_strategy.as_str() {
        "time" => HedgeStrategy::TimeBased {
            interval: chrono::Duration::hours(args.hedge_interval_hours as i64),
        },
        "delta" => HedgeStrategy::DeltaThreshold {
            threshold: args.delta_threshold,
        },
        "gamma" => HedgeStrategy::GammaDollar {
            threshold: args.delta_threshold * 100.0,  // Scale for dollar gamma
        },
        _ => HedgeStrategy::None,
    };

    HedgeConfig {
        strategy,
        max_rehedges: args.max_rehedges,
        min_hedge_size: 1,
        transaction_cost_per_share: Decimal::try_from(args.hedge_cost_per_share)
            .unwrap_or(Decimal::ZERO),
        contract_multiplier: 100,
    }
}
```

---

### Phase 5: Analytics & Reporting

**Duration:** ~2 hours

#### 5.1 Hedging Analytics

**File:** `cs-backtest/src/hedging_analytics.rs` (NEW)

```rust
use cs_domain::{HedgePosition, StraddleResult};
use rust_decimal::Decimal;

/// Compare hedged vs unhedged performance
pub struct HedgingComparison {
    pub unhedged_pnl: Decimal,
    pub hedged_pnl: Decimal,
    pub hedge_contribution: Decimal,  // hedged_pnl - unhedged_pnl
    pub hedge_cost: Decimal,
    pub num_rehedges: usize,
    pub hedge_efficiency: f64,  // hedge_contribution / hedge_cost
}

impl HedgingComparison {
    pub fn from_result(result: &StraddleResult) -> Option<Self> {
        let hedge_pos = result.hedge_position.as_ref()?;
        let hedge_pnl = result.hedge_pnl?;
        let total_pnl = result.total_pnl_with_hedge?;

        let unhedged_pnl = result.pnl;
        let hedge_contribution = total_pnl - unhedged_pnl;
        let hedge_cost = hedge_pos.total_cost;

        let hedge_efficiency = if hedge_cost > Decimal::ZERO {
            (hedge_contribution / hedge_cost).try_into().unwrap_or(0.0)
        } else {
            0.0
        };

        Some(Self {
            unhedged_pnl,
            hedged_pnl: total_pnl,
            hedge_contribution,
            hedge_cost,
            num_rehedges: hedge_pos.rehedge_count(),
            hedge_efficiency,
        })
    }
}

/// Aggregate statistics for hedging backtest
pub struct HedgingStats {
    pub total_trades: usize,
    pub hedged_trades: usize,
    pub avg_rehedges_per_trade: f64,
    pub total_hedge_cost: Decimal,
    pub total_hedge_pnl: Decimal,
    pub avg_hedge_efficiency: f64,
    pub hedged_sharpe: f64,
    pub unhedged_sharpe: f64,
}
```

---

## Part 4: File Structure Summary

### New Files
```
cs-domain/src/
├── hedging.rs              # HedgeAction, HedgePosition, HedgeConfig, HedgeStrategy

cs-backtest/src/
├── hedging_executor.rs     # HedgingExecutor wrapper
├── hedging_analytics.rs    # HedgingComparison, HedgingStats
```

### Modified Files
```
cs-domain/src/
├── lib.rs                  # Export hedging module
├── entities.rs             # Add hedge fields to StraddleResult

cs-backtest/src/
├── lib.rs                  # Export hedging modules
├── timing_strategy.rs      # Add rehedge_times() method
├── backtest_use_case.rs    # Integrate hedging executor

cs-cli/src/
├── cli_args.rs             # Add --hedge-* arguments
├── main.rs                 # Build HedgeConfig from args
```

---

## Part 5: Usage Examples

### Basic Delta Hedging
```bash
./target/release/cs backtest \
  --symbols PENG \
  --spread straddle \
  --straddle-entry-days 10 \
  --straddle-exit-days 2 \
  --hedge \
  --hedge-strategy delta \
  --delta-threshold 0.10
```

### Time-Based Hedging (Daily)
```bash
./target/release/cs backtest \
  --symbols AAPL \
  --spread straddle \
  --hedge \
  --hedge-strategy time \
  --hedge-interval-hours 24
```

### Hedging with Cost Analysis
```bash
./target/release/cs backtest \
  --symbols NVDA \
  --spread straddle \
  --hedge \
  --hedge-strategy delta \
  --delta-threshold 0.05 \
  --hedge-cost-per-share 0.02 \
  --max-rehedges 10
```

---

## Part 6: Testing Strategy

### Unit Tests
1. `HedgeConfig::should_rehedge()` - Test all strategies
2. `HedgeConfig::shares_to_hedge()` - Test calculation accuracy
3. `HedgePosition::calculate_pnl()` - Test P&L computation
4. `TimingStrategy::rehedge_times()` - Test schedule generation

### Integration Tests
1. Full backtest with hedging enabled
2. Compare hedged vs unhedged results
3. Verify hedge P&L matches manual calculation
4. Test edge cases (no rehedges needed, max rehedges hit)

### Validation
1. Run on historical data where manual hedge calculation is known
2. Compare with theoretical gamma scalping P&L
3. Verify transaction costs are correctly deducted

---

## Part 7: Future Enhancements

### Near-Term
- [ ] Support hedging for Calendar Spreads (more complex delta)
- [ ] Support hedging for Iron Butterflies
- [ ] Add bid-ask spread simulation

### Medium-Term
- [ ] Gamma scalping optimization (optimal hedge frequency)
- [ ] Machine learning for hedge timing
- [ ] Real-time hedging signals

### Long-Term
- [ ] Multi-asset hedging (correlated underlyings)
- [ ] Vega hedging with VIX options
- [ ] Portfolio-level delta management

---

## Part 8: Advanced Enhancements - Deep Dive

### 8.1 Gamma Scalping Optimization

#### 8.1.1 The Fundamental Tradeoff

Gamma scalping involves a fundamental tradeoff between **theta decay** (time value erosion) and **gamma profits** (profits from rehedging):

```
Net P&L = Gamma Profits - Theta Decay - Transaction Costs

Where:
  Gamma Profits = ½ × Γ × Σ(ΔS_i)²     (realized variance)
  Theta Decay   = θ × T                  (time held)
  Transaction   = cost_per_trade × N     (number of rehedges)
```

**Breakeven Condition:**
For gamma scalping to be profitable, realized volatility must exceed implied volatility:
```
σ_realized > σ_implied
```

This is because:
- Theta decay is priced based on implied volatility
- Gamma profits depend on actual (realized) price movements

#### 8.1.2 Optimal Hedge Frequency

The optimal hedge frequency balances:
1. **Too frequent**: High transaction costs eat into gamma profits
2. **Too infrequent**: Miss gamma scalping opportunities, larger delta exposure

**Theoretical Optimal Interval (Continuous Hedging):**
```
Optimal Δt = √(2 × cost / (Γ × σ² × S²))

Where:
  cost = transaction cost per hedge
  Γ    = position gamma
  σ    = volatility (annualized)
  S    = spot price
```

**Practical Implementation:**

```rust
/// Calculate optimal hedge interval based on position characteristics
pub struct OptimalHedgeCalculator {
    transaction_cost: f64,      // Cost per round-trip hedge
    gamma: f64,                 // Position gamma
    volatility: f64,            // Implied or realized vol
    spot: f64,                  // Current spot price
}

impl OptimalHedgeCalculator {
    /// Theoretical optimal interval in seconds
    pub fn optimal_interval_seconds(&self) -> f64 {
        let gamma_dollar = self.gamma * self.spot * self.spot;
        let vol_per_second = self.volatility / (252.0 * 6.5 * 3600.0).sqrt();

        // Optimal Δt = sqrt(2 * cost / (Γ$ * σ²))
        (2.0 * self.transaction_cost / (gamma_dollar * vol_per_second.powi(2))).sqrt()
    }

    /// Optimal delta band (hedge when |Δ| exceeds this)
    pub fn optimal_delta_band(&self) -> f64 {
        // Derived from Whalley-Wilmott asymptotic expansion
        let lambda = self.transaction_cost / (self.gamma * self.spot);
        (3.0 * lambda / 2.0).powf(1.0 / 3.0) * self.volatility.powf(2.0 / 3.0)
    }

    /// Expected gamma P&L per day (assuming continuous hedging)
    pub fn expected_daily_gamma_pnl(&self, realized_vol: f64) -> f64 {
        // E[Gamma P&L] = ½ × Γ × S² × σ² × Δt
        0.5 * self.gamma * self.spot.powi(2) * realized_vol.powi(2) / 252.0
    }

    /// Breakeven realized volatility
    pub fn breakeven_vol(&self, theta: f64) -> f64 {
        // Solve: ½ × Γ × S² × σ² = |θ| + costs
        let gamma_dollar = self.gamma * self.spot * self.spot;
        ((2.0 * theta.abs()) / gamma_dollar).sqrt() * (252.0_f64).sqrt()
    }
}
```

#### 8.1.3 Variance Drain Analysis

**Variance Drain** is the phenomenon where discrete hedging captures less variance than continuous hedging:

```
Variance Captured (discrete) < Variance Captured (continuous)

Efficiency = Captured / Theoretical = 1 - O(Δt)
```

**Simulation Framework:**

```rust
/// Analyze variance drain across different hedge frequencies
pub struct VarianceDrainAnalyzer {
    price_path: Vec<f64>,       // Historical prices
    timestamps: Vec<DateTime>,   // Corresponding times
    gamma: f64,                  // Position gamma
}

impl VarianceDrainAnalyzer {
    /// Calculate captured variance for given hedge interval
    pub fn captured_variance(&self, hedge_interval: Duration) -> f64 {
        let mut captured = 0.0;
        let mut last_hedge_idx = 0;

        for (i, ts) in self.timestamps.iter().enumerate() {
            if *ts - self.timestamps[last_hedge_idx] >= hedge_interval {
                let price_move = self.price_path[i] - self.price_path[last_hedge_idx];
                captured += 0.5 * self.gamma * price_move.powi(2);
                last_hedge_idx = i;
            }
        }
        captured
    }

    /// Calculate theoretical continuous variance
    pub fn theoretical_variance(&self) -> f64 {
        let mut total = 0.0;
        for i in 1..self.price_path.len() {
            let move_sq = (self.price_path[i] - self.price_path[i-1]).powi(2);
            total += 0.5 * self.gamma * move_sq;
        }
        total
    }

    /// Find optimal interval that maximizes (captured - costs)
    pub fn optimize_interval(
        &self,
        cost_per_hedge: f64,
        min_interval: Duration,
        max_interval: Duration,
    ) -> (Duration, f64) {
        let mut best_interval = min_interval;
        let mut best_pnl = f64::NEG_INFINITY;

        let mut interval = min_interval;
        while interval <= max_interval {
            let captured = self.captured_variance(interval);
            let num_hedges = self.count_hedges(interval);
            let net_pnl = captured - (num_hedges as f64 * cost_per_hedge);

            if net_pnl > best_pnl {
                best_pnl = net_pnl;
                best_interval = interval;
            }
            interval = interval + Duration::minutes(15);
        }
        (best_interval, best_pnl)
    }
}
```

#### 8.1.4 Adaptive Hedging Strategies

**Volatility-Adjusted Hedging:**
Hedge more frequently when volatility is high, less when low:

```rust
pub enum AdaptiveHedgeStrategy {
    /// Scale hedge frequency with realized vol
    VolatilityScaled {
        base_interval: Duration,
        vol_lookback: usize,      // Periods to calculate realized vol
        vol_multiplier: f64,      // How much to adjust
    },

    /// Widen/narrow delta band based on vol regime
    VolatilityBands {
        base_threshold: f64,
        low_vol_multiplier: f64,  // Widen band in low vol (e.g., 1.5)
        high_vol_multiplier: f64, // Narrow band in high vol (e.g., 0.7)
    },

    /// Time-of-day aware (more activity at open/close)
    IntradayAware {
        open_interval: Duration,   // First hour
        midday_interval: Duration, // 11am-2pm
        close_interval: Duration,  // Last hour
    },
}
```

**P&L Attribution by Hedge:**

```rust
/// Detailed P&L breakdown for gamma scalping analysis
pub struct GammaScalpingPnL {
    pub gross_gamma_pnl: f64,      // Total from rehedges
    pub theta_decay: f64,          // Time value lost
    pub transaction_costs: f64,    // All hedge costs
    pub net_pnl: f64,              // Gross - theta - costs

    // Per-hedge breakdown
    pub hedge_pnls: Vec<HedgePnL>,

    // Efficiency metrics
    pub variance_capture_ratio: f64,  // Realized / Theoretical
    pub cost_per_variance: f64,       // Costs / Variance captured
    pub sharpe_ratio: f64,            // Risk-adjusted return
}

pub struct HedgePnL {
    pub timestamp: DateTime,
    pub spot_at_hedge: f64,
    pub delta_before: f64,
    pub shares_traded: i32,
    pub realized_pnl: f64,           // Contribution to total
    pub cumulative_pnl: f64,         // Running total
}
```

---

### 8.2 Machine Learning for Hedge Timing

#### 8.2.1 Problem Formulation

**Objective:** Learn when to hedge to maximize risk-adjusted returns

**Decision:** At each timestep t, choose action a ∈ {HOLD, HEDGE}

**State Space (Features):**
```
s_t = {
    # Position features
    delta_t,           # Current position delta
    gamma_t,           # Current position gamma
    theta_t,           # Current theta decay rate
    days_to_expiry,    # Time remaining

    # Market features
    spot_t,            # Current spot price
    realized_vol_t,    # Recent realized volatility
    implied_vol_t,     # Current IV
    vol_spread_t,      # IV - RV (vol risk premium)

    # Technical features
    spot_momentum,     # Price trend
    vol_momentum,      # Vol trend
    time_since_hedge,  # Seconds since last hedge

    # Microstructure
    bid_ask_spread,    # Current spread
    volume,            # Recent volume
}
```

**Reward Function:**
```
r_t = {
    gamma_pnl_t - theta_t - cost_t,  if HEDGE
    gamma_pnl_t - theta_t,            if HOLD
}
```

#### 8.2.2 Supervised Learning Approach

**Label Generation (Hindsight Optimal):**

```rust
/// Generate training labels using hindsight analysis
pub struct HedgeLabelGenerator {
    lookahead_window: Duration,
    min_profitable_move: f64,
}

impl HedgeLabelGenerator {
    /// Determine if hedging at time t would have been profitable
    pub fn should_have_hedged(
        &self,
        price_path: &[f64],
        t: usize,
        delta_t: f64,
        hedge_cost: f64,
    ) -> bool {
        // Look at price path over next N periods
        let future_prices = &price_path[t..t + self.lookahead_window];

        // Calculate what hedge P&L would have been
        let hedge_pnl: f64 = future_prices.iter()
            .map(|p| -delta_t * (p - price_path[t]))
            .sum();

        // Calculate what unhedged P&L would have been
        let unhedged_pnl = 0.0;  // No action

        // Hedge if it would have been profitable after costs
        (hedge_pnl - hedge_cost) > unhedged_pnl + self.min_profitable_move
    }

    /// Generate dataset from historical trades
    pub fn generate_dataset(
        &self,
        trades: &[HistoricalTrade],
    ) -> Dataset {
        let mut features = Vec::new();
        let mut labels = Vec::new();

        for trade in trades {
            for (t, snapshot) in trade.snapshots.iter().enumerate() {
                let feature_vec = self.extract_features(snapshot, trade);
                let label = self.should_have_hedged(
                    &trade.price_path,
                    t,
                    snapshot.delta,
                    trade.hedge_cost,
                );

                features.push(feature_vec);
                labels.push(label as u8);
            }
        }

        Dataset { features, labels }
    }
}
```

**Model Architecture Options:**

| Model | Pros | Cons | Best For |
|-------|------|------|----------|
| Logistic Regression | Interpretable, fast | Linear only | Baseline |
| Random Forest | Handles non-linearity, feature importance | Can overfit | Production |
| XGBoost | Best accuracy, handles imbalance | Black box | Research |
| LSTM | Captures temporal patterns | Needs lots of data | Intraday |
| Transformer | State-of-art for sequences | Complex, slow | Future work |

**Feature Importance Analysis:**

```rust
/// Analyze which features matter most for hedge timing
pub struct FeatureImportance {
    pub feature_names: Vec<String>,
    pub importances: Vec<f64>,
    pub std_devs: Vec<f64>,  // Stability across folds
}

impl FeatureImportance {
    /// Expected top features for hedge timing
    pub fn expected_ranking() -> Vec<&'static str> {
        vec![
            "delta_magnitude",      // How far from neutral
            "vol_spread",           // IV vs RV
            "time_since_hedge",     // Avoid over-trading
            "realized_vol_short",   // Recent activity
            "gamma_dollar",         // Exposure size
            "days_to_expiry",       // Urgency
            "spot_momentum",        // Trend
            "bid_ask_spread",       // Cost to hedge
        ]
    }
}
```

#### 8.2.3 Reinforcement Learning Approach

**Environment Definition:**

```rust
/// RL environment for hedge timing
pub struct HedgeTimingEnv {
    // State
    position: OptionPosition,
    current_step: usize,
    price_path: Vec<f64>,

    // Tracking
    cumulative_pnl: f64,
    hedge_count: usize,
    shares_held: i32,

    // Config
    transaction_cost: f64,
    max_steps: usize,
}

impl HedgeTimingEnv {
    /// Take action and return (next_state, reward, done)
    pub fn step(&mut self, action: HedgeAction) -> (State, f64, bool) {
        let old_spot = self.price_path[self.current_step];
        self.current_step += 1;
        let new_spot = self.price_path[self.current_step];

        // Calculate reward
        let spot_pnl = self.shares_held as f64 * (new_spot - old_spot);
        let theta_cost = self.position.theta / (252.0 * 6.5 * 12.0); // Per 5-min
        let hedge_cost = if action == HedgeAction::Hedge {
            self.transaction_cost
        } else {
            0.0
        };

        let reward = spot_pnl - theta_cost - hedge_cost;

        // Execute action
        if action == HedgeAction::Hedge {
            let delta = self.calculate_delta(new_spot);
            self.shares_held = (-delta * 100.0).round() as i32;
            self.hedge_count += 1;
        }

        // Update position Greeks
        self.position.update(new_spot, self.current_step);

        let done = self.current_step >= self.max_steps;
        let state = self.get_state();

        (state, reward, done)
    }
}
```

**DQN Agent Structure:**

```python
# Pseudocode for DQN implementation
class HedgeDQN:
    def __init__(self, state_dim, hidden_dim=64):
        self.q_network = Sequential([
            Dense(hidden_dim, activation='relu'),
            Dense(hidden_dim, activation='relu'),
            Dense(2)  # Q-values for [HOLD, HEDGE]
        ])
        self.target_network = clone(self.q_network)
        self.memory = ReplayBuffer(capacity=100000)

    def select_action(self, state, epsilon):
        if random() < epsilon:
            return random_choice([HOLD, HEDGE])
        q_values = self.q_network(state)
        return argmax(q_values)

    def train_step(self, batch_size=64):
        states, actions, rewards, next_states, dones = self.memory.sample(batch_size)

        # Compute target Q-values
        next_q = self.target_network(next_states).max(dim=1)
        targets = rewards + gamma * next_q * (1 - dones)

        # Update Q-network
        predictions = self.q_network(states).gather(actions)
        loss = mse_loss(predictions, targets)
        loss.backward()
        optimizer.step()
```

#### 8.2.4 Training Pipeline

```rust
/// End-to-end ML training pipeline for hedge timing
pub struct HedgeMLPipeline {
    // Data
    historical_trades: Vec<HistoricalTrade>,
    train_ratio: f64,

    // Model
    model_type: ModelType,
    hyperparameters: HyperParams,

    // Evaluation
    backtest_config: BacktestConfig,
}

impl HedgeMLPipeline {
    pub fn run(&self) -> PipelineResults {
        // 1. Generate features and labels
        let dataset = self.generate_dataset();

        // 2. Train/test split (temporal, not random!)
        let (train, test) = dataset.temporal_split(self.train_ratio);

        // 3. Train model with cross-validation
        let model = self.train_with_cv(&train);

        // 4. Evaluate on hold-out test set
        let test_metrics = self.evaluate(&model, &test);

        // 5. Full backtest comparison
        let backtest_results = self.compare_strategies(&model);

        PipelineResults {
            model,
            test_metrics,
            backtest_results,
            feature_importance: model.feature_importance(),
        }
    }

    fn compare_strategies(&self, model: &TrainedModel) -> StrategyComparison {
        let strategies = vec![
            ("No Hedge", HedgeStrategy::None),
            ("Fixed Interval (1h)", HedgeStrategy::TimeBased { interval: Duration::hours(1) }),
            ("Delta Threshold (0.10)", HedgeStrategy::DeltaThreshold { threshold: 0.10 }),
            ("ML Model", HedgeStrategy::MLModel { model: model.clone() }),
        ];

        let mut results = Vec::new();
        for (name, strategy) in strategies {
            let backtest = self.run_backtest(strategy);
            results.push(StrategyResult {
                name,
                total_pnl: backtest.total_pnl,
                sharpe: backtest.sharpe_ratio,
                max_drawdown: backtest.max_drawdown,
                num_hedges: backtest.total_hedges,
            });
        }

        StrategyComparison { results }
    }
}
```

#### 8.2.5 Live Deployment Considerations

**Model Serving Architecture:**

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ Market Data     │────▶│ Feature Engine   │────▶│ ML Model        │
│ (spot, IV, vol) │     │ (real-time calc) │     │ (inference)     │
└─────────────────┘     └──────────────────┘     └────────┬────────┘
                                                          │
                                                          ▼
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ Execution       │◀────│ Risk Checks      │◀────│ Hedge Decision  │
│ (broker API)    │     │ (position limits)│     │ (HEDGE/HOLD)    │
└─────────────────┘     └──────────────────┘     └─────────────────┘
```

**Monitoring & Safeguards:**

```rust
pub struct MLHedgeMonitor {
    // Performance tracking
    predictions: RollingWindow<Prediction>,
    actual_outcomes: RollingWindow<Outcome>,

    // Drift detection
    feature_distributions: HashMap<String, Distribution>,
    model_confidence: RollingWindow<f64>,

    // Safeguards
    max_hedges_per_hour: usize,
    min_confidence_threshold: f64,
    fallback_strategy: HedgeStrategy,
}

impl MLHedgeMonitor {
    /// Check if model should be trusted for this decision
    pub fn should_use_ml(&self, prediction: &Prediction) -> bool {
        // Check confidence
        if prediction.confidence < self.min_confidence_threshold {
            return false;
        }

        // Check for feature drift
        if self.detect_feature_drift() {
            return false;
        }

        // Check recent accuracy
        if self.recent_accuracy() < 0.45 {  // Worse than random
            return false;
        }

        true
    }

    /// Detect if input features have drifted from training distribution
    fn detect_feature_drift(&self) -> bool {
        // Kolmogorov-Smirnov test or similar
        // Returns true if distribution has shifted significantly
        false  // Placeholder
    }
}
```

---

### 8.3 Additional Advanced Features

#### 8.3.1 Multi-Asset Delta Hedging

For portfolios with correlated underlyings:

```rust
/// Portfolio-level delta management
pub struct PortfolioDeltaHedger {
    positions: Vec<OptionPosition>,
    correlations: CorrelationMatrix,
    beta_exposures: Vec<f64>,  // Beta to SPY/market
}

impl PortfolioDeltaHedger {
    /// Calculate portfolio delta in dollar terms
    pub fn portfolio_delta(&self) -> f64 {
        self.positions.iter()
            .map(|p| p.delta * p.spot * p.multiplier as f64)
            .sum()
    }

    /// Hedge with index instead of individual stocks
    pub fn hedge_with_index(&self, index_price: f64) -> i32 {
        let portfolio_beta: f64 = self.positions.iter()
            .zip(&self.beta_exposures)
            .map(|(p, beta)| p.delta * p.spot * beta)
            .sum();

        // Shares of index to hedge
        (-portfolio_beta / index_price).round() as i32
    }
}
```

#### 8.3.2 Vega Hedging

```rust
/// Combined delta-vega hedging
pub struct DeltaVegaHedger {
    position: OptionPosition,
    vega_hedge_instrument: Option<VIXOption>,  // VIX calls/puts
}

impl DeltaVegaHedger {
    /// Hedge both delta and vega exposure
    pub fn dual_hedge(&self) -> HedgeOrders {
        let delta_shares = (-self.position.delta * 100.0).round() as i32;

        let vega_contracts = if let Some(vix) = &self.vega_hedge_instrument {
            // Vega hedge: buy VIX calls if short vega, sell if long
            let vix_vega = vix.vega;
            (-self.position.vega / vix_vega).round() as i32
        } else {
            0
        };

        HedgeOrders {
            stock_shares: delta_shares,
            vix_contracts: vega_contracts,
        }
    }
}
```

#### 8.3.3 Transaction Cost Optimization

```rust
/// Smart order routing for hedge execution
pub struct HedgeOrderOptimizer {
    venues: Vec<ExecutionVenue>,
    current_spread: f64,
    urgency: HedgeUrgency,
}

impl HedgeOrderOptimizer {
    /// Choose optimal execution strategy
    pub fn optimize_execution(&self, shares: i32) -> ExecutionPlan {
        match self.urgency {
            HedgeUrgency::Immediate => {
                // Market order, accept spread
                ExecutionPlan::MarketOrder { shares }
            }
            HedgeUrgency::Normal => {
                // Limit order at mid, wait up to 1 min
                ExecutionPlan::LimitOrder {
                    shares,
                    limit_price: self.mid_price(),
                    timeout: Duration::minutes(1),
                }
            }
            HedgeUrgency::Patient => {
                // TWAP over 5 minutes
                ExecutionPlan::TWAP {
                    shares,
                    duration: Duration::minutes(5),
                    slices: 10,
                }
            }
        }
    }
}
```

---

### 8.4 Implementation Roadmap for Advanced Features

| Feature | Complexity | Dependencies | Priority |
|---------|------------|--------------|----------|
| Optimal hedge frequency calculator | Medium | Basic hedging | High |
| Variance drain analysis | Medium | Historical data | High |
| Adaptive hedging (vol-scaled) | Medium | Realized vol calc | Medium |
| Supervised ML (Random Forest) | High | Feature pipeline | Medium |
| RL (DQN) | Very High | RL framework | Low |
| Portfolio hedging | High | Multi-position tracking | Low |
| Vega hedging | High | VIX data integration | Low |

---

## Appendix: Mathematical Reference

### Delta of a Straddle
```
Δ_straddle = Δ_call + Δ_put
           = N(d1) + (N(d1) - 1)
           = 2·N(d1) - 1
```
At-the-money: `Δ ≈ 0` (calls ≈ +0.50, puts ≈ -0.50)

### Gamma Scalping P&L
```
P&L_gamma = ½ × Γ × (ΔS)²
```
Where:
- `Γ` = position gamma
- `ΔS` = spot price change

### Hedge P&L
```
P&L_hedge = Σ (shares_i × (S_exit - S_hedge_i))
```
Where each hedge `i` contributes based on entry price vs exit price.

### Total P&L with Hedging
```
P&L_total = P&L_option + P&L_hedge - costs
```
