# P&L Computation Research and Proposed Solution

**Date:** 2026-01-09
**Status:** Research Document
**Related Issue:** `specs/pnl_computation_issue.txt`

## Executive Summary

The current P&L system has a fundamental mismatch between percentage returns (Mean Return: +23.50%) and dollar P&L (Total P&L: -$13,668.71). This occurs because:
- **Mean Return** averages percentage returns without weighting by position size
- **Total P&L** sums dollar amounts directly

This document proposes a comprehensive P&L accounting system that:
1. Properly tracks capital allocation per trade
2. Calculates weighted returns (capital-weighted, not simple average)
3. Handles debit/credit trades correctly
4. Supports margin calculations for stocks and options
5. Provides industry-standard performance metrics

---

## 1. Problem Analysis

### 1.1 Current Behavior

From `cs-backtest/src/backtest_use_case.rs`:

```rust
// Mean Return: Simple average of percentage returns
pub fn mean_return(&self) -> f64 {
    let returns = self.pnl_pcts();
    returns.iter().sum::<f64>() / returns.len() as f64
}

// Total P&L: Sum of dollar amounts
pub fn total_pnl(&self) -> Decimal {
    self.results.iter().map(|r| r.pnl()).sum()
}
```

### 1.2 Why They Diverge

**Example with varying position sizes:**

| Trade | Initial Debit | P&L ($) | Return (%) |
|-------|---------------|---------|------------|
| A     | $75           | +$7.50  | +10%       |
| B     | $170          | -$85.00 | -50%       |
| C     | $50           | +$25.00 | +50%       |

**Current calculations:**
- Mean Return = (10% + (-50%) + 50%) / 3 = **+3.33%**
- Total P&L = $7.50 - $85.00 + $25.00 = **-$52.50**

**The issue:** Trade B has a larger position (higher debit), so its -50% loss dominates the dollar P&L, while the simple average treats all trades equally.

### 1.3 Real-World Implications

In the straddle backtest:
- **High IV stocks** = expensive straddles = bigger losses when wrong
- **Low IV stocks** = cheap straddles = smaller profits when right
- Winners are on smaller positions, losers are on larger positions
- Result: Positive mean return but negative total P&L

---

## 2. Industry Standards

### 2.1 Return Calculation Methods

#### Simple Return (Current)
```
R_simple = mean(r_1, r_2, ..., r_n)
```
**Problem:** Does not account for position size. A 10% return on $100 is treated the same as 10% on $10,000.

#### Capital-Weighted Return (Proposed)
```
R_weighted = sum(capital_i * r_i) / sum(capital_i)
```
**Solution:** Weights each return by the capital deployed.

#### Time-Weighted Return (TWR)
Used by portfolio managers. Eliminates the effect of cash flows.
```
TWR = [(1 + R_1) * (1 + R_2) * ... * (1 + R_n)] - 1
```
**Use case:** Evaluating strategy performance independent of position sizing.

#### Money-Weighted Return (MWR/IRR)
Accounts for timing and size of cash flows.
```
Find r such that: sum(CF_t / (1+r)^t) = 0
```
**Use case:** Measuring actual investor experience.

### 2.2 Capital Tracking Methods

**Source:** [Interactive Brokers Position and P&L](https://www.ibkrguides.com/traderworkstation/position-and-pnl.htm)

| Metric | Formula |
|--------|---------|
| Unrealized P&L | (Current Price - Avg Cost) × Quantity |
| Realized P&L | (Sale Price - Avg Cost) × Quantity - Fees |
| Daily P&L | Mark-to-market change from prior close |

### 2.3 Margin Requirements (CBOE)

**Source:** [CBOE Strategy-based Margin](https://www.cboe.com/us/options/strategy_based_margin)

| Position Type | Margin Requirement |
|---------------|-------------------|
| Long Options (≤9 months) | 100% of premium |
| Long Options (>9 months) | 75% of premium |
| Short Naked Equity | 100% proceeds + 20% underlying - OTM amount (min 10%) |
| Short Naked Index | 100% proceeds + 15% underlying - OTM amount (min 10%) |
| Debit Spreads | Net debit in full |
| Credit Spreads | Max loss - credit received |
| Straddles (long) | Sum of both legs |
| Stock Hedge (long) | 100% of stock value (or 50% on margin) |
| Stock Hedge (short) | 150% of stock value (Reg-T) |

---

## 3. Proposed Solution Architecture

### 3.1 Core Domain Types

```rust
// cs-domain/src/accounting/mod.rs

/// Represents capital required for a trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalRequirement {
    /// Initial margin/debit required to enter
    pub initial_requirement: Decimal,

    /// Maintenance margin (may differ from initial)
    pub maintenance_requirement: Decimal,

    /// Method used to calculate requirement
    pub calculation_method: CapitalCalculationMethod,

    /// Breakdown by component
    pub breakdown: CapitalBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapitalCalculationMethod {
    /// Full debit for long options
    LongOptionDebit,
    /// Strategy-based margin for spreads
    StrategyBasedMargin,
    /// CBOE Reg-T for equities
    RegTMargin,
    /// Portfolio margin (if supported)
    PortfolioMargin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalBreakdown {
    /// Option premium (debit/credit)
    pub option_premium: Decimal,
    /// Stock hedge capital (if any)
    pub hedge_capital: Decimal,
    /// Additional margin for shorts (if any)
    pub short_margin: Decimal,
    /// Total buying power reduction
    pub total_bpr: Decimal,
}

/// Complete trade accounting record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeAccounting {
    /// Capital required to enter
    pub capital_required: CapitalRequirement,

    /// Entry cash flow (negative = paid, positive = received)
    pub entry_cash_flow: Decimal,

    /// Exit cash flow (negative = paid, positive = received)
    pub exit_cash_flow: Decimal,

    /// Transaction costs (always negative)
    pub transaction_costs: Decimal,

    /// Realized P&L (locked in at close)
    pub realized_pnl: Decimal,

    /// Return on capital deployed
    pub return_on_capital: f64,
}

impl TradeAccounting {
    /// Calculate return on capital
    pub fn calculate_return(&self) -> f64 {
        if self.capital_required.initial_requirement.is_zero() {
            return 0.0;
        }
        (self.realized_pnl / self.capital_required.initial_requirement)
            .to_f64()
            .unwrap_or(0.0)
    }
}
```

### 3.2 Trade Type Accounting

```rust
// cs-domain/src/accounting/trade_types.rs

/// Debit trade (pay premium upfront)
/// Examples: Long straddle, long butterfly, calendar spread (usually)
pub struct DebitTradeAccounting {
    /// Premium paid (positive number, will be recorded as negative cash flow)
    pub premium_paid: Decimal,
    /// Value received at exit
    pub exit_value: Decimal,
    /// Multiplier (100 for options)
    pub multiplier: u32,
}

impl DebitTradeAccounting {
    pub fn to_trade_accounting(&self) -> TradeAccounting {
        let entry_cash_flow = -self.premium_paid * Decimal::from(self.multiplier);
        let exit_cash_flow = self.exit_value * Decimal::from(self.multiplier);
        let realized_pnl = exit_cash_flow + entry_cash_flow; // exit - entry

        TradeAccounting {
            capital_required: CapitalRequirement {
                initial_requirement: self.premium_paid * Decimal::from(self.multiplier),
                maintenance_requirement: self.premium_paid * Decimal::from(self.multiplier),
                calculation_method: CapitalCalculationMethod::LongOptionDebit,
                breakdown: CapitalBreakdown {
                    option_premium: self.premium_paid * Decimal::from(self.multiplier),
                    hedge_capital: Decimal::ZERO,
                    short_margin: Decimal::ZERO,
                    total_bpr: self.premium_paid * Decimal::from(self.multiplier),
                },
            },
            entry_cash_flow,
            exit_cash_flow,
            transaction_costs: Decimal::ZERO,
            realized_pnl,
            return_on_capital: 0.0, // Calculate after construction
        }
    }
}

/// Credit trade (receive premium upfront, margin required)
/// Examples: Short straddle, iron butterfly, credit spreads
pub struct CreditTradeAccounting {
    /// Premium received (positive number)
    pub premium_received: Decimal,
    /// Cost to close (positive = paid to close)
    pub close_cost: Decimal,
    /// Margin required (capital at risk)
    pub margin_required: Decimal,
    /// Multiplier (100 for options)
    pub multiplier: u32,
}

impl CreditTradeAccounting {
    pub fn to_trade_accounting(&self) -> TradeAccounting {
        let entry_cash_flow = self.premium_received * Decimal::from(self.multiplier);
        let exit_cash_flow = -self.close_cost * Decimal::from(self.multiplier);
        let realized_pnl = entry_cash_flow + exit_cash_flow; // credit - close_cost

        TradeAccounting {
            capital_required: CapitalRequirement {
                initial_requirement: self.margin_required,
                maintenance_requirement: self.margin_required,
                calculation_method: CapitalCalculationMethod::StrategyBasedMargin,
                breakdown: CapitalBreakdown {
                    option_premium: self.premium_received * Decimal::from(self.multiplier),
                    hedge_capital: Decimal::ZERO,
                    short_margin: self.margin_required,
                    total_bpr: self.margin_required,
                },
            },
            entry_cash_flow,
            exit_cash_flow,
            transaction_costs: Decimal::ZERO,
            realized_pnl,
            return_on_capital: 0.0,
        }
    }
}

/// Hedged trade (options + stock hedge)
pub struct HedgedTradeAccounting {
    /// Base option accounting
    pub option_accounting: Box<dyn Into<TradeAccounting>>,
    /// Hedge position accounting
    pub hedge_accounting: HedgeAccounting,
}

pub struct HedgeAccounting {
    /// Long shares capital (full value or margin)
    pub long_capital: Decimal,
    /// Short shares margin (typically 150% Reg-T)
    pub short_margin: Decimal,
    /// Realized P&L from hedging
    pub realized_pnl: Decimal,
    /// Transaction costs from hedge trades
    pub transaction_costs: Decimal,
}
```

### 3.3 Margin Calculator

```rust
// cs-domain/src/accounting/margin.rs

/// CBOE Strategy-Based Margin Calculator
pub struct MarginCalculator {
    /// Stock margin requirement (default 50% for Reg-T long, 150% for short)
    pub stock_margin_long: f64,
    pub stock_margin_short: f64,
    /// Use portfolio margin rules
    pub use_portfolio_margin: bool,
}

impl Default for MarginCalculator {
    fn default() -> Self {
        Self {
            stock_margin_long: 0.50,   // 50% for Reg-T long
            stock_margin_short: 1.50,  // 150% for Reg-T short
            use_portfolio_margin: false,
        }
    }
}

impl MarginCalculator {
    /// Calculate margin for a long option position
    pub fn long_option_margin(&self, premium: Decimal, dte: u32) -> Decimal {
        if dte <= 270 {
            // 9 months or less: full premium
            premium
        } else {
            // More than 9 months: 75% of premium
            premium * Decimal::from_str("0.75").unwrap()
        }
    }

    /// Calculate margin for a naked short option
    pub fn naked_short_margin(
        &self,
        premium: Decimal,
        underlying_price: Decimal,
        strike: Decimal,
        is_call: bool,
    ) -> Decimal {
        let otm_amount = if is_call {
            (strike - underlying_price).max(Decimal::ZERO)
        } else {
            (underlying_price - strike).max(Decimal::ZERO)
        };

        // 100% proceeds + 20% underlying - OTM amount
        let standard = premium + (underlying_price * dec!(0.20)) - otm_amount;
        // Minimum: 10% of underlying + premium
        let minimum = (underlying_price * dec!(0.10)) + premium;

        standard.max(minimum)
    }

    /// Calculate margin for a defined-risk spread
    pub fn spread_margin(
        &self,
        is_debit: bool,
        net_premium: Decimal,
        max_loss: Decimal,
    ) -> Decimal {
        if is_debit {
            // Debit spread: pay the full debit
            net_premium.abs()
        } else {
            // Credit spread: max loss - credit received
            max_loss - net_premium.abs()
        }
    }

    /// Calculate capital for stock hedge
    pub fn stock_hedge_capital(
        &self,
        shares: i32,
        price: Decimal,
    ) -> Decimal {
        let notional = Decimal::from(shares.abs()) * price;
        if shares > 0 {
            // Long stock: use margin rate
            notional * Decimal::try_from(self.stock_margin_long).unwrap()
        } else {
            // Short stock: use short margin rate
            notional * Decimal::try_from(self.stock_margin_short).unwrap()
        }
    }
}
```

### 3.4 Statistics Calculator

```rust
// cs-domain/src/accounting/statistics.rs

/// Comprehensive trade statistics
pub struct TradeStatistics {
    // Basic counts
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,

    // Dollar P&L
    pub total_pnl: Decimal,
    pub total_option_pnl: Decimal,
    pub total_hedge_pnl: Decimal,
    pub total_transaction_costs: Decimal,

    // Returns (properly weighted)
    pub simple_mean_return: f64,        // Current: simple average
    pub capital_weighted_return: f64,   // NEW: weighted by capital deployed
    pub time_weighted_return: f64,      // NEW: TWR for strategy evaluation
    pub money_weighted_return: f64,     // NEW: MWR/IRR for investor experience

    // Risk metrics
    pub std_deviation: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown: Decimal,
    pub win_rate: f64,

    // Winner/Loser analysis
    pub avg_winner_dollars: Decimal,
    pub avg_winner_pct: f64,
    pub avg_loser_dollars: Decimal,
    pub avg_loser_pct: f64,
    pub profit_factor: f64,             // NEW: gross profit / gross loss

    // Capital efficiency
    pub total_capital_deployed: Decimal,
    pub peak_capital_required: Decimal,
    pub return_on_peak_capital: f64,
}

impl TradeStatistics {
    pub fn from_trades(trades: &[TradeAccounting]) -> Self {
        if trades.is_empty() {
            return Self::default();
        }

        let total_trades = trades.len();
        let winning_trades = trades.iter()
            .filter(|t| t.realized_pnl > Decimal::ZERO)
            .count();
        let losing_trades = trades.iter()
            .filter(|t| t.realized_pnl < Decimal::ZERO)
            .count();

        // Dollar P&L
        let total_pnl: Decimal = trades.iter().map(|t| t.realized_pnl).sum();

        // Capital deployed
        let total_capital_deployed: Decimal = trades.iter()
            .map(|t| t.capital_required.initial_requirement)
            .sum();

        // Simple mean return (current behavior)
        let simple_mean_return = {
            let returns: Vec<f64> = trades.iter()
                .map(|t| t.return_on_capital)
                .collect();
            returns.iter().sum::<f64>() / returns.len() as f64
        };

        // Capital-weighted return (NEW - the fix!)
        let capital_weighted_return = {
            let weighted_sum: f64 = trades.iter()
                .map(|t| {
                    let capital = t.capital_required.initial_requirement
                        .to_f64().unwrap_or(0.0);
                    let return_val = t.return_on_capital;
                    capital * return_val
                })
                .sum();
            let total_cap = total_capital_deployed.to_f64().unwrap_or(1.0);
            if total_cap > 0.0 { weighted_sum / total_cap } else { 0.0 }
        };

        // Time-weighted return (geometric linking)
        let time_weighted_return = {
            let product: f64 = trades.iter()
                .map(|t| 1.0 + t.return_on_capital)
                .product();
            product.powf(1.0 / trades.len() as f64) - 1.0
        };

        // Win rate
        let win_rate = winning_trades as f64 / total_trades as f64;

        // Profit factor
        let gross_profit: Decimal = trades.iter()
            .filter(|t| t.realized_pnl > Decimal::ZERO)
            .map(|t| t.realized_pnl)
            .sum();
        let gross_loss: Decimal = trades.iter()
            .filter(|t| t.realized_pnl < Decimal::ZERO)
            .map(|t| t.realized_pnl.abs())
            .sum();
        let profit_factor = if !gross_loss.is_zero() {
            (gross_profit / gross_loss).to_f64().unwrap_or(0.0)
        } else {
            f64::INFINITY
        };

        // Standard deviation of returns
        let returns: Vec<f64> = trades.iter()
            .map(|t| t.return_on_capital)
            .collect();
        let variance = returns.iter()
            .map(|r| (r - capital_weighted_return).powi(2))
            .sum::<f64>() / (returns.len() - 1).max(1) as f64;
        let std_deviation = variance.sqrt();

        // Sharpe ratio (annualized)
        let sharpe_ratio = if std_deviation > 0.0 {
            capital_weighted_return / std_deviation * 16.0 // sqrt(252)
        } else {
            0.0
        };

        // Winner/Loser analysis
        let winners: Vec<_> = trades.iter()
            .filter(|t| t.realized_pnl > Decimal::ZERO)
            .collect();
        let losers: Vec<_> = trades.iter()
            .filter(|t| t.realized_pnl < Decimal::ZERO)
            .collect();

        let avg_winner_dollars = if !winners.is_empty() {
            winners.iter().map(|t| t.realized_pnl).sum::<Decimal>()
                / Decimal::from(winners.len())
        } else {
            Decimal::ZERO
        };

        let avg_loser_dollars = if !losers.is_empty() {
            losers.iter().map(|t| t.realized_pnl).sum::<Decimal>()
                / Decimal::from(losers.len())
        } else {
            Decimal::ZERO
        };

        let avg_winner_pct = if !winners.is_empty() {
            winners.iter().map(|t| t.return_on_capital).sum::<f64>()
                / winners.len() as f64
        } else {
            0.0
        };

        let avg_loser_pct = if !losers.is_empty() {
            losers.iter().map(|t| t.return_on_capital).sum::<f64>()
                / losers.len() as f64
        } else {
            0.0
        };

        Self {
            total_trades,
            winning_trades,
            losing_trades,
            total_pnl,
            total_option_pnl: Decimal::ZERO, // To be filled by caller
            total_hedge_pnl: Decimal::ZERO,  // To be filled by caller
            total_transaction_costs: trades.iter()
                .map(|t| t.transaction_costs)
                .sum(),
            simple_mean_return,
            capital_weighted_return,
            time_weighted_return,
            money_weighted_return: 0.0, // IRR calculation is complex, defer
            std_deviation,
            sharpe_ratio,
            max_drawdown: Decimal::ZERO, // To be calculated separately
            win_rate,
            avg_winner_dollars,
            avg_winner_pct,
            avg_loser_dollars,
            avg_loser_pct,
            profit_factor,
            total_capital_deployed,
            peak_capital_required: total_capital_deployed, // Simplified
            return_on_peak_capital: if !total_capital_deployed.is_zero() {
                (total_pnl / total_capital_deployed).to_f64().unwrap_or(0.0)
            } else {
                0.0
            },
        }
    }
}
```

---

## 4. Integration with Existing Code

### 4.1 TradeResult Enhancement

Each trade result type needs to include `TradeAccounting`:

```rust
// In cs-domain/src/entities.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StraddleResult {
    // ... existing fields ...

    /// Accounting data (NEW)
    pub accounting: Option<TradeAccounting>,
}

impl StraddleResult {
    /// Get capital-weighted return
    pub fn return_on_capital(&self) -> f64 {
        self.accounting
            .as_ref()
            .map(|a| a.return_on_capital)
            .unwrap_or_else(|| {
                // Fallback to pnl_pct if no accounting
                (self.pnl_pct / dec!(100)).to_f64().unwrap_or(0.0)
            })
    }
}
```

### 4.2 BacktestResult Enhancement

```rust
// In cs-backtest/src/backtest_use_case.rs

impl<R: TradeResultMethods + HasAccounting> BacktestResult<R> {
    /// Capital-weighted mean return (NEW - correct calculation)
    pub fn capital_weighted_return(&self) -> f64 {
        let weighted_sum: f64 = self.results.iter()
            .filter_map(|r| r.accounting())
            .map(|a| {
                let capital = a.capital_required.initial_requirement
                    .to_f64().unwrap_or(0.0);
                capital * a.return_on_capital
            })
            .sum();

        let total_capital: f64 = self.results.iter()
            .filter_map(|r| r.accounting())
            .map(|a| a.capital_required.initial_requirement
                .to_f64().unwrap_or(0.0))
            .sum();

        if total_capital > 0.0 {
            weighted_sum / total_capital
        } else {
            0.0
        }
    }

    /// Simple mean return (preserved for comparison)
    pub fn simple_mean_return(&self) -> f64 {
        // Current implementation, renamed
        let returns = self.pnl_pcts();
        returns.iter().sum::<f64>() / returns.len() as f64
    }
}
```

### 4.3 Display Update

```rust
// In cs-cli/src/output/backtest.rs

pub fn display_metrics<R: TradeResultMethods + HasAccounting>(result: &BacktestResult<R>) {
    println!("
    +---------------------------+--------------------+
    | Metric                    | Value              |
    +---------------------------+--------------------+
    | Win Rate                  | {:.2}%             |
    +---------------------------+--------------------+
    | Total P&L                 | ${:.2}             |
    +---------------------------+--------------------+
    | Avg P&L per Trade         | ${:.2}             |
    +---------------------------+--------------------+
    |                           |                    |
    +---------------------------+--------------------+
    | Capital-Weighted Return   | {:.2}%             |  <- NEW (correct)
    +---------------------------+--------------------+
    | Simple Mean Return        | {:.2}%             |  <- OLD (for reference)
    +---------------------------+--------------------+
    | Return on Total Capital   | {:.2}%             |  <- NEW
    +---------------------------+--------------------+
    ...",
        result.win_rate() * 100.0,
        result.total_pnl(),
        result.avg_pnl(),
        result.capital_weighted_return() * 100.0,
        result.simple_mean_return() * 100.0,
        result.return_on_capital() * 100.0,
    );
}
```

---

## 5. Campaign System Comparison

The campaign system (`RollingResult`) already has better capital tracking:

**What campaign does right:**
- `total_option_premium` - tracks capital deployed
- `peak_hedge_capital` - tracks hedge capital
- `return_on_capital` - calculated on total capital
- `annualized_return` - time-adjusted

**What campaign is missing:**
- Per-roll capital-weighted returns
- Proper margin calculations for credit trades
- Transaction cost consistency
- Leverage tracking

**Recommendation:** Port the campaign's `CapitalSummary` pattern to individual trades.

---

## 6. Implementation Plan

### Phase 1: Core Types (2-3 days)
- [ ] Create `cs-domain/src/accounting/` module
- [ ] Implement `CapitalRequirement`, `TradeAccounting`
- [ ] Implement `MarginCalculator`

### Phase 2: Trade Integration (3-4 days)
- [ ] Add `accounting` field to all result types
- [ ] Update execution code to populate accounting
- [ ] Add `HasAccounting` trait

### Phase 3: Statistics (2-3 days)
- [ ] Implement `TradeStatistics`
- [ ] Add `capital_weighted_return()` to `BacktestResult`
- [ ] Add `profit_factor`, other new metrics

### Phase 4: Display & Output (1-2 days)
- [ ] Update CLI output to show new metrics
- [ ] Update JSON output format
- [ ] Add backwards compatibility notes

### Phase 5: Campaign Alignment (2-3 days)
- [ ] Align campaign P&L with new accounting
- [ ] Port improvements back to `RollingResult`
- [ ] Ensure consistent calculations

---

## 7. Key Decisions Required

1. **Default capital for debit trades:** Full debit or margined (75%)?
   - Recommendation: Full debit for conservatism

2. **Stock hedge margin:** Reg-T (50%) or full value?
   - Recommendation: Configurable, default Reg-T

3. **Transaction costs:** Include in capital or separate?
   - Recommendation: Separate, like campaign

4. **Simple mean return:** Keep or remove?
   - Recommendation: Keep for comparison, rename to `simple_mean_return`

5. **New primary metric name:** What to call capital-weighted return?
   - Options: "Weighted Return", "Return on Capital", "Capital-Weighted Return"
   - Recommendation: "Return on Capital" (ROC)

---

## 8. References

### Industry Standards
- [IBKR Position and P&L](https://www.ibkrguides.com/traderworkstation/position-and-pnl.htm)
- [CBOE Strategy-based Margin](https://www.cboe.com/us/options/strategy_based_margin)
- [CBOE Portfolio Margining](https://www.cboe.com/us/options/portfolio_margining_rules/)

### Academic
- CFA Institute: GIPS (Global Investment Performance Standards)
- Time-Weighted vs Money-Weighted Returns

### Internal
- `cs-domain/src/entities/rolling_result.rs` - Campaign capital tracking
- `cs-backtest/src/session_executor.rs` - Session P&L extraction
- `cs-analytics/src/pnl_attribution.rs` - Greeks-based P&L breakdown
