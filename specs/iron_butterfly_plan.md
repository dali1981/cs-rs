# Iron Butterfly Strategy Implementation Plan

## Overview

Implement iron butterfly as an alternative to calendar spread for earnings IV crush plays. An iron butterfly is a short straddle with protective wings, providing defined risk.

## Structure

```
Iron Butterfly:
  - Short ATM Call  (sell)
  - Short ATM Put   (sell)
  - Long OTM Call   (buy, wing)
  - Long OTM Put    (buy, wing)

All legs same expiration (matches current calendar short leg expiration)
```

**P&L Profile**:
- Max Profit: Net credit received (when stock at strike at expiration)
- Max Loss: Wing width - credit (defined)
- Breakeven: Strike ± credit

---

## Implementation Phases

### Phase 1: Domain Entities

**File: `cs-domain/src/entities.rs`**

```rust
/// Iron butterfly = short straddle + long wings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronButterfly {
    pub short_call: OptionLeg,
    pub short_put: OptionLeg,
    pub long_call: OptionLeg,  // Upper wing
    pub long_put: OptionLeg,   // Lower wing
}

impl IronButterfly {
    pub fn new(
        short_call: OptionLeg,
        short_put: OptionLeg,
        long_call: OptionLeg,
        long_put: OptionLeg,
    ) -> Result<Self, ValidationError> {
        // Validate same symbol
        // Validate same expiration for all legs
        // Validate short_call.strike == short_put.strike (ATM)
        // Validate long_call.strike > short_call.strike
        // Validate long_put.strike < short_put.strike
        // Validate option types correct
        Ok(Self { short_call, short_put, long_call, long_put })
    }

    pub fn symbol(&self) -> &str { &self.short_call.symbol }
    pub fn center_strike(&self) -> Strike { self.short_call.strike }
    pub fn upper_strike(&self) -> Strike { self.long_call.strike }
    pub fn lower_strike(&self) -> Strike { self.long_put.strike }
    pub fn expiration(&self) -> NaiveDate { self.short_call.expiration }

    /// Wing width (assumes symmetric wings)
    pub fn wing_width(&self) -> Decimal {
        self.upper_strike().value() - self.center_strike().value()
    }
}
```

**Result Type:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronButterflyResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub center_strike: Strike,
    pub upper_strike: Strike,
    pub lower_strike: Strike,
    pub expiration: NaiveDate,
    pub wing_width: Decimal,

    // Entry (CREDIT received)
    pub entry_time: DateTime<Utc>,
    pub short_call_entry: Decimal,
    pub short_put_entry: Decimal,
    pub long_call_entry: Decimal,
    pub long_put_entry: Decimal,
    pub entry_credit: Decimal,  // (short_call + short_put) - (long_call + long_put)

    // Exit (cost to close)
    pub exit_time: DateTime<Utc>,
    pub short_call_exit: Decimal,
    pub short_put_exit: Decimal,
    pub long_call_exit: Decimal,
    pub long_put_exit: Decimal,
    pub exit_cost: Decimal,

    // P&L
    pub pnl: Decimal,           // entry_credit - exit_cost
    pub pnl_pct: Decimal,       // pnl / entry_credit * 100
    pub max_loss: Decimal,      // wing_width - entry_credit

    // Greeks at entry (net position)
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    // IV at entry/exit (use short call IV as reference)
    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
    pub iv_crush: Option<f64>,  // iv_entry - iv_exit

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move: f64,
    pub spot_move_pct: f64,

    // Breakeven analysis
    pub breakeven_up: f64,
    pub breakeven_down: f64,
    pub within_breakeven: bool,  // Was exit spot within breakevens?

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}
```

### Phase 2: Strategy Selection

**File: `cs-domain/src/strategies/iron_butterfly.rs`**

```rust
pub struct IronButterflyStrategy {
    pub wing_width: Decimal,  // e.g., $10
    pub min_dte: i32,
    pub max_dte: i32,
}

impl IronButterflyStrategy {
    pub fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<IronButterfly, StrategyError> {
        // 1. Select expiration (same logic as calendar short leg)
        let expiration = self.select_expiration(event, &chain_data.expirations)?;

        // 2. Find ATM strike for center
        let center = self.find_atm_strike(spot, &chain_data.strikes)?;

        // 3. Calculate wing strikes
        let upper = Strike::new(center.value() + self.wing_width)?;
        let lower = Strike::new(center.value() - self.wing_width)?;

        // 4. Snap to available strikes
        let upper = self.snap_to_strike(upper, &chain_data.strikes, true)?;
        let lower = self.snap_to_strike(lower, &chain_data.strikes, false)?;

        // 5. Build legs
        let short_call = OptionLeg::new(event.symbol.clone(), center, expiration, OptionType::Call);
        let short_put = OptionLeg::new(event.symbol.clone(), center, expiration, OptionType::Put);
        let long_call = OptionLeg::new(event.symbol.clone(), upper, expiration, OptionType::Call);
        let long_put = OptionLeg::new(event.symbol.clone(), lower, expiration, OptionType::Put);

        IronButterfly::new(short_call, short_put, long_call, long_put)
    }

    fn snap_to_strike(
        &self,
        target: Strike,
        available: &[Strike],
        round_up: bool,
    ) -> Result<Strike, StrategyError> {
        // Find closest available strike >= target (if round_up) or <= target
        available.iter()
            .filter(|s| if round_up { **s >= target } else { **s <= target })
            .min_by_key(|s| (s.value() - target.value()).abs())
            .copied()
            .ok_or(StrategyError::NoStrikes)
    }
}
```

### Phase 3: Pricer

**File: `cs-backtest/src/iron_butterfly_pricer.rs`**

```rust
pub struct IronButterflyPricing {
    pub short_call: LegPricing,
    pub short_put: LegPricing,
    pub long_call: LegPricing,
    pub long_put: LegPricing,
    pub net_credit: Decimal,  // Total credit received
}

impl IronButterflyPricer {
    pub fn price(
        &self,
        butterfly: &IronButterfly,
        chain_df: &DataFrame,
        spot: f64,
        time: DateTime<Utc>,
    ) -> Result<IronButterflyPricing, PricingError> {
        // Price all 4 legs using existing price_leg logic
        let short_call = self.price_leg(&butterfly.short_call, ...)?;
        let short_put = self.price_leg(&butterfly.short_put, ...)?;
        let long_call = self.price_leg(&butterfly.long_call, ...)?;
        let long_put = self.price_leg(&butterfly.long_put, ...)?;

        // Net credit = sell - buy
        let net_credit = (short_call.price + short_put.price)
                       - (long_call.price + long_put.price);

        Ok(IronButterflyPricing {
            short_call, short_put, long_call, long_put, net_credit
        })
    }
}
```

### Phase 4: Executor

**File: `cs-backtest/src/iron_butterfly_executor.rs`**

```rust
impl IronButterflyExecutor {
    pub async fn execute_trade(
        &self,
        butterfly: &IronButterfly,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> IronButterflyResult {
        // Get spot prices
        let entry_spot = self.equity_repo.get_spot_price(...).await?;
        let exit_spot = self.equity_repo.get_spot_price(...).await?;

        // Price at entry
        let entry_pricing = self.pricer.price(butterfly, entry_chain, entry_spot, entry_time)?;

        // Validate minimum credit
        if entry_pricing.net_credit < Decimal::new(50, 2) {  // $0.50 min
            return Err("Credit too small");
        }

        // Price at exit
        let exit_pricing = self.pricer.price(butterfly, exit_chain, exit_spot, exit_time)?;

        // P&L = credit received - cost to close
        let pnl = entry_pricing.net_credit - exit_pricing.net_credit;
        let pnl_pct = (pnl / entry_pricing.net_credit) * Decimal::from(100);

        // Calculate max loss
        let max_loss = butterfly.wing_width() - entry_pricing.net_credit;

        // Breakeven levels
        let center_f64: f64 = butterfly.center_strike().into();
        let credit_f64: f64 = entry_pricing.net_credit.try_into().unwrap_or(0.0);
        let breakeven_up = center_f64 + credit_f64;
        let breakeven_down = center_f64 - credit_f64;

        // Net greeks (short straddle - long strangle)
        let net_delta = compute_net_delta(&entry_pricing);
        let net_gamma = compute_net_gamma(&entry_pricing);
        let net_theta = compute_net_theta(&entry_pricing);
        let net_vega = compute_net_vega(&entry_pricing);

        IronButterflyResult { ... }
    }
}

fn compute_net_delta(p: &IronButterflyPricing) -> Option<f64> {
    match (p.short_call.greeks, p.short_put.greeks, p.long_call.greeks, p.long_put.greeks) {
        (Some(sc), Some(sp), Some(lc), Some(lp)) => {
            // Short positions: negate delta
            // Short call delta is positive, short put delta is negative
            // For a short position, we flip the sign
            Some(-sc.delta - sp.delta + lc.delta + lp.delta)
        }
        _ => None
    }
}

fn compute_net_gamma(p: &IronButterflyPricing) -> Option<f64> {
    // Gamma is always positive, short positions = negative gamma exposure
    match (p.short_call.greeks, p.short_put.greeks, p.long_call.greeks, p.long_put.greeks) {
        (Some(sc), Some(sp), Some(lc), Some(lp)) => {
            Some(-sc.gamma - sp.gamma + lc.gamma + lp.gamma)
        }
        _ => None
    }
}
```

### Phase 5: CLI Integration

**File: `cs-cli/src/cli_args.rs`**

```rust
#[derive(ValueEnum, Clone, Debug)]
pub enum StrategyMode {
    Calendar,
    IronButterfly,
}

#[derive(Args)]
pub struct BacktestArgs {
    #[arg(long, default_value = "calendar")]
    pub strategy: StrategyMode,

    /// Wing width for iron butterfly (default: $10)
    #[arg(long, default_value = "10")]
    pub wing_width: f64,
}
```

### Phase 6: Config

**File: `cs-backtest/src/config.rs`**

```rust
pub struct BacktestConfig {
    // ... existing fields
    pub strategy: StrategyConfig,
}

pub enum StrategyConfig {
    Calendar(CalendarConfig),
    IronButterfly(IronButterflyConfig),
}

pub struct IronButterflyConfig {
    pub wing_width: Decimal,
    pub min_dte: i32,
    pub max_dte: i32,
    pub min_credit: Decimal,  // Minimum credit to enter
}
```

---

## Implementation Order

1. **Entities** - `IronButterfly`, `IronButterflyResult`
2. **Strategy** - `IronButterflyStrategy` with strike selection
3. **Pricer** - 4-leg pricing
4. **Executor** - P&L calculation
5. **Config** - Strategy configuration
6. **CLI** - `--strategy iron-butterfly --wing-width 10`
7. **Results** - Parquet schema

---

## Testing Plan

1. **Unit Tests**
   - Entity validation (strike ordering, option types)
   - P&L calculation (credit - exit cost)
   - Breakeven calculation

2. **Integration Tests**
   - Single symbol backtest
   - Compare vs calendar spread on same events

3. **Validation Scenarios**
   - Stock at strike (max profit)
   - Stock at breakeven (zero P&L)
   - Stock beyond wing (max loss)

---

## CLI Usage

```bash
# Calendar spread (existing)
./target/release/cs backtest --start 2024-01-01 --end 2024-06-30

# Iron butterfly with $10 wings
./target/release/cs backtest --strategy iron-butterfly --wing-width 10 \
    --start 2024-01-01 --end 2024-06-30

# Iron butterfly with $5 wings (tighter, higher credit, less room)
./target/release/cs backtest --strategy iron-butterfly --wing-width 5 \
    --start 2024-01-01 --end 2024-06-30
```
