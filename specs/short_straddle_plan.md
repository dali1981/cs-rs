# Short Straddle Strategy Implementation Plan

## Overview

Replace calendar spread with a **short straddle** on the same expiration as the current short leg. This tests the hypothesis that selling volatility directly (without the long leg hedge) captures more IV crush premium around earnings.

## Strategy Comparison

| Aspect | Calendar Spread | Short Straddle |
|--------|-----------------|----------------|
| **Structure** | Short near-term + Long far-term | Short call + Short put |
| **Strikes** | Same strike, different expirations | Same strike, same expiration |
| **Entry** | Pay debit (long_price - short_price) | Receive credit (call_price + put_price) |
| **Risk** | Limited (long leg caps loss) | Unlimited (naked short on both sides) |
| **Profit source** | IV crush + theta on short leg | IV crush + theta decay |
| **Max profit** | When short expires ATM | When stock stays at strike |
| **Breakeven** | Complex (depends on back month) | Strike ± total premium received |

## Architectural Changes

### Phase 1: Domain Entities

**File: `cs-domain/src/entities.rs`**

```rust
/// Short straddle = short call + short put at same strike/expiration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortStraddle {
    pub call_leg: OptionLeg,  // Short call
    pub put_leg: OptionLeg,   // Short put
}

impl ShortStraddle {
    pub fn new(call: OptionLeg, put: OptionLeg) -> Result<Self, ValidationError> {
        // Validate: same symbol
        if call.symbol != put.symbol {
            return Err(ValidationError::SymbolMismatch(...));
        }
        // Validate: same strike
        if call.strike != put.strike {
            return Err(ValidationError::StrikeMismatch { call: ..., put: ... });
        }
        // Validate: same expiration
        if call.expiration != put.expiration {
            return Err(ValidationError::ExpirationMismatch { ... });
        }
        // Validate: correct option types
        if call.option_type != OptionType::Call {
            return Err(ValidationError::InvalidOptionType("Expected call"));
        }
        if put.option_type != OptionType::Put {
            return Err(ValidationError::InvalidOptionType("Expected put"));
        }
        Ok(Self { call_leg: call, put_leg: put })
    }

    pub fn symbol(&self) -> &str { &self.call_leg.symbol }
    pub fn strike(&self) -> Strike { self.call_leg.strike }
    pub fn expiration(&self) -> NaiveDate { self.call_leg.expiration }
    pub fn dte(&self, from: NaiveDate) -> i32 { self.call_leg.dte(from) }
}
```

**New Result Type:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StraddleResult {
    // Identification
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub strike: Strike,
    pub expiration: NaiveDate,

    // Entry (CREDIT - receive premium)
    pub entry_time: DateTime<Utc>,
    pub call_entry_price: Decimal,
    pub put_entry_price: Decimal,
    pub entry_credit: Decimal,  // call_price + put_price (positive = received)

    // Exit (cost to close)
    pub exit_time: DateTime<Utc>,
    pub call_exit_price: Decimal,
    pub put_exit_price: Decimal,
    pub exit_cost: Decimal,  // call_price + put_price (positive = paid)

    // P&L = entry_credit - exit_cost (profit when options expire worthless)
    pub pnl: Decimal,
    pub pnl_per_contract: Decimal,
    pub pnl_pct: Decimal,  // pnl / entry_credit * 100

    // Greeks at entry (combined straddle greeks)
    pub call_delta: Option<f64>,
    pub put_delta: Option<f64>,
    pub net_delta: Option<f64>,     // call_delta + put_delta (≈0 for ATM)
    pub call_gamma: Option<f64>,
    pub put_gamma: Option<f64>,
    pub net_gamma: Option<f64>,     // Short gamma (risk)
    pub call_theta: Option<f64>,
    pub put_theta: Option<f64>,
    pub net_theta: Option<f64>,     // Positive theta (profit)
    pub call_vega: Option<f64>,
    pub put_vega: Option<f64>,
    pub net_vega: Option<f64>,      // Negative vega (short vol)

    // IV at entry/exit
    pub iv_call_entry: Option<f64>,
    pub iv_put_entry: Option<f64>,
    pub iv_call_exit: Option<f64>,
    pub iv_put_exit: Option<f64>,

    // P&L Attribution
    pub delta_pnl: Option<Decimal>,
    pub gamma_pnl: Option<Decimal>,
    pub theta_pnl: Option<Decimal>,
    pub vega_pnl: Option<Decimal>,
    pub unexplained_pnl: Option<Decimal>,

    // Spot prices
    pub spot_at_entry: f64,
    pub spot_at_exit: f64,

    // Breakeven levels (for analysis)
    pub breakeven_up: f64,    // strike + entry_credit
    pub breakeven_down: f64,  // strike - entry_credit

    // Status
    pub success: bool,
    pub failure_reason: Option<FailureReason>,
}

impl StraddleResult {
    pub fn is_winner(&self) -> bool {
        self.success && self.pnl > Decimal::ZERO
    }

    /// Spot move required to hit breakeven (absolute)
    pub fn breakeven_move(&self) -> f64 {
        (self.breakeven_up - self.strike.to_f64()).abs()
    }
}
```

**New ValidationError variant:**

```rust
pub enum ValidationError {
    // ... existing variants
    StrikeMismatch { call: Strike, put: Strike },
    InvalidOptionType(String),
}
```

### Phase 2: Strategy Selection

**File: `cs-domain/src/strategies/straddle.rs` (NEW)**

```rust
/// ATM straddle strategy - selects ATM strike for straddle
pub struct ATMStraddleStrategy {
    criteria: StraddleSelectionCriteria,
}

#[derive(Debug, Clone)]
pub struct StraddleSelectionCriteria {
    pub min_dte: i32,       // Minimum DTE (avoid gamma risk)
    pub max_dte: i32,       // Maximum DTE
    pub target_delta: Option<f64>,  // Optional: select by delta instead of spot
}

impl ATMStraddleStrategy {
    pub fn new(criteria: StraddleSelectionCriteria) -> Self {
        Self { criteria }
    }

    pub fn select(
        &self,
        event: &EarningsEvent,
        spot: &SpotPrice,
        chain_data: &OptionChainData,
    ) -> Result<ShortStraddle, StrategyError> {
        // 1. Find valid expirations matching criteria
        let valid_exp = self.select_expiration(event, &chain_data.expirations)?;

        // 2. Find ATM strike (closest to spot)
        let strike = self.find_atm_strike(spot, &chain_data.strikes)?;

        // 3. Build call and put legs
        let call = OptionLeg::new(
            event.symbol.clone(),
            strike,
            valid_exp,
            OptionType::Call,
        );
        let put = OptionLeg::new(
            event.symbol.clone(),
            strike,
            valid_exp,
            OptionType::Put,
        );

        ShortStraddle::new(call, put).map_err(Into::into)
    }

    fn select_expiration(
        &self,
        event: &EarningsEvent,
        expirations: &[NaiveDate],
    ) -> Result<NaiveDate, StrategyError> {
        // Use the FIRST expiration >= earnings_date with DTE in range
        // This matches the current short leg selection logic
        expirations.iter()
            .filter(|&exp| {
                let dte = (*exp - event.earnings_date).num_days() as i32;
                dte >= self.criteria.min_dte && dte <= self.criteria.max_dte
            })
            .next()
            .copied()
            .ok_or(StrategyError::NoExpirations)
    }

    fn find_atm_strike(
        &self,
        spot: &SpotPrice,
        strikes: &[Strike],
    ) -> Result<Strike, StrategyError> {
        strikes.iter()
            .min_by_key(|s| {
                let diff = s.value() - spot.value();
                (diff.abs() * Decimal::from(1000)).to_i64().unwrap_or(i64::MAX)
            })
            .copied()
            .ok_or(StrategyError::NoStrikes)
    }
}
```

### Phase 3: Straddle Pricer

**File: `cs-backtest/src/straddle_pricer.rs` (NEW)**

```rust
/// Pricing result for a straddle
#[derive(Debug, Clone)]
pub struct StraddlePricing {
    pub call_leg: LegPricing,
    pub put_leg: LegPricing,
    pub total_premium: Decimal,  // call + put (credit received)
}

pub struct StraddlePricer {
    // Reuse SpreadPricer internals for single-leg pricing
    inner: SpreadPricer,
}

impl StraddlePricer {
    pub fn price_straddle(
        &self,
        straddle: &ShortStraddle,
        chain_df: &DataFrame,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
    ) -> Result<StraddlePricing, PricingError> {
        // Price call leg
        let call_pricing = self.inner.price_leg(
            &straddle.strike(),
            straddle.expiration(),
            OptionType::Call,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface.as_ref(),
            pricing_provider.as_ref(),
        )?;

        // Price put leg
        let put_pricing = self.inner.price_leg(
            &straddle.strike(),
            straddle.expiration(),
            OptionType::Put,
            chain_df,
            spot_price,
            pricing_time,
            iv_surface.as_ref(),
            pricing_provider.as_ref(),
        )?;

        // Total premium = call + put (credit for short straddle)
        let total_premium = call_pricing.price + put_pricing.price;

        Ok(StraddlePricing {
            call_leg: call_pricing,
            put_leg: put_pricing,
            total_premium,
        })
    }
}
```

### Phase 4: Trade Executor

**File: `cs-backtest/src/straddle_executor.rs` (NEW)**

```rust
pub struct StraddleExecutor<O, E> {
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    pricer: StraddlePricer,
    max_entry_iv: Option<f64>,
}

impl<O, E> StraddleExecutor<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    pub async fn execute_trade(
        &self,
        straddle: &ShortStraddle,
        event: &EarningsEvent,
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
    ) -> StraddleResult {
        // 1. Get spot prices at entry/exit
        let entry_spot = self.equity_repo.get_spot_price(...).await?;
        let exit_spot = self.equity_repo.get_spot_price(...).await?;

        // 2. Get option chain data
        let entry_chain = self.options_repo.get_option_bars(...).await?;
        let exit_chain = self.options_repo.get_option_bars(...).await?;

        // 3. Price at entry (receive premium)
        let entry_pricing = self.pricer.price_straddle(
            straddle, &entry_chain, entry_spot, entry_time
        )?;

        // 4. Validate minimum premium
        let min_premium = Decimal::new(10, 2); // $0.10 minimum
        if entry_pricing.total_premium < min_premium {
            return Err("Premium too small");
        }

        // 5. Price at exit (pay to close)
        let exit_pricing = self.pricer.price_straddle(
            straddle, &exit_chain, exit_spot, exit_time
        )?;

        // 6. Calculate P&L (SHORT position: profit when premium decreases)
        // P&L = entry_credit - exit_cost
        let pnl = entry_pricing.total_premium - exit_pricing.total_premium;
        let pnl_pct = (pnl / entry_pricing.total_premium) * 100;

        // 7. Calculate breakeven levels
        let strike_f64: f64 = straddle.strike().into();
        let premium_f64: f64 = entry_pricing.total_premium.to_f64().unwrap_or(0.0);
        let breakeven_up = strike_f64 + premium_f64;
        let breakeven_down = strike_f64 - premium_f64;

        // 8. Build result
        StraddleResult { ... }
    }
}
```

### Phase 5: P&L Attribution

**File: `cs-domain/src/services/pnl_calculator.rs`**

Add new function for straddle P&L attribution:

```rust
pub fn calculate_straddle_pnl_attribution(
    call_greeks: &Greeks,
    put_greeks: &Greeks,
    spot_change: f64,
    call_iv_change: f64,
    put_iv_change: f64,
    days_held: f64,
    total_pnl: Decimal,
) -> PnLAttribution {
    // Net greeks for short straddle (negative signs for short position)
    let net_delta = -(call_greeks.delta + put_greeks.delta);
    let net_gamma = -(call_greeks.gamma + put_greeks.gamma);
    let net_theta = -(call_greeks.theta + put_greeks.theta);  // Positive (we earn theta)
    let call_vega = -call_greeks.vega;  // Negative (we're short vega)
    let put_vega = -put_greeks.vega;

    // Attribution
    let delta_pnl = net_delta * spot_change;
    let gamma_pnl = 0.5 * net_gamma * spot_change.powi(2);
    let theta_pnl = net_theta * days_held;

    // Vega P&L: separate for call and put (different IV changes possible)
    let vega_pnl = call_vega * call_iv_change * 100.0
                 + put_vega * put_iv_change * 100.0;

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl.to_f64().unwrap_or(0.0) - explained;

    PnLAttribution {
        delta: Decimal::try_from(delta_pnl).unwrap_or_default(),
        gamma: Decimal::try_from(gamma_pnl).unwrap_or_default(),
        theta: Decimal::try_from(theta_pnl).unwrap_or_default(),
        vega: Decimal::try_from(vega_pnl).unwrap_or_default(),
        unexplained: Decimal::try_from(unexplained).unwrap_or_default(),
    }
}
```

### Phase 6: Backtest Use Case

**File: `cs-backtest/src/backtest_use_case.rs`**

Add straddle backtest mode:

```rust
pub enum StrategyType {
    Calendar(CalendarStrategyConfig),
    Straddle(StraddleStrategyConfig),  // NEW
}

pub struct StraddleStrategyConfig {
    pub min_dte: i32,
    pub max_dte: i32,
}

// In run_backtest:
match strategy_type {
    StrategyType::Calendar(config) => {
        // Existing calendar logic
    }
    StrategyType::Straddle(config) => {
        let strategy = ATMStraddleStrategy::new(config.into());
        let executor = StraddleExecutor::new(...);

        for event in events {
            let straddle = strategy.select(&event, &spot, &chain_data)?;
            let result = executor.execute_trade(&straddle, &event, entry, exit).await;
            results.push(result);
        }
    }
}
```

### Phase 7: CLI Integration

**File: `cs-cli/src/cli_args.rs`**

```rust
#[derive(ValueEnum, Clone, Debug)]
pub enum StrategyMode {
    Calendar,
    Straddle,  // NEW
}

#[derive(Args)]
pub struct BacktestArgs {
    // ... existing args

    #[arg(long, default_value = "calendar")]
    pub strategy: StrategyMode,
}
```

### Phase 8: Results Repository

**File: `cs-domain/src/infrastructure/parquet_results_repo.rs`**

Add schema and serialization for `StraddleResult`.

---

## Implementation Order

1. **Domain Entities** (Phase 1)
   - Add `ShortStraddle` entity
   - Add `StraddleResult` type
   - Add validation errors

2. **Strategy** (Phase 2)
   - Create `ATMStraddleStrategy`
   - Add to mod.rs exports

3. **Pricer** (Phase 3)
   - Create `StraddlePricer`
   - Reuse `SpreadPricer::price_leg` internally

4. **Executor** (Phase 4)
   - Create `StraddleExecutor`
   - Implement P&L calculation for credit spread

5. **P&L Attribution** (Phase 5)
   - Add straddle-specific attribution function

6. **Backtest Use Case** (Phase 6)
   - Add `StrategyType::Straddle`
   - Wire up strategy + executor

7. **CLI** (Phase 7)
   - Add `--strategy straddle` flag

8. **Results Storage** (Phase 8)
   - Add parquet schema for straddle results

---

## Key Differences in P&L Calculation

### Calendar Spread (Debit)
```
entry_cost = long_price - short_price  (pay)
exit_value = long_price - short_price  (receive)
pnl = exit_value - entry_cost
pnl_pct = pnl / entry_cost * 100
```

### Short Straddle (Credit)
```
entry_credit = call_price + put_price  (receive)
exit_cost = call_price + put_price     (pay)
pnl = entry_credit - exit_cost
pnl_pct = pnl / entry_credit * 100
```

---

## Risk Considerations

**Unlimited Risk**: Unlike calendar spreads, short straddles have unlimited risk on both sides. The stock can move infinitely up (call side loss) or to zero (put side loss).

**Margin Requirements**: Short straddles require significant margin. Consider adding a `margin_requirement` field to track capital efficiency.

**Gamma Risk**: At expiration, gamma explodes for ATM options. The current earnings strategy exits before expiration, but this is still a concern.

**Earnings Move**: If the stock moves more than the premium received, the trade is a loser. Consider tracking:
- `earnings_move_pct`: Actual stock move %
- `implied_move`: Premium / strike (expected move)
- `move_ratio`: actual / implied

---

## Testing Plan

1. **Unit Tests**
   - `ShortStraddle::new` validation
   - `StraddleResult` construction
   - P&L calculation (credit spread math)
   - Breakeven calculation

2. **Integration Tests**
   - End-to-end backtest on single symbol
   - Compare calendar vs straddle on same events

3. **Validation**
   - Run both strategies on 2023-2024 data
   - Compare win rate, avg P&L, drawdowns
