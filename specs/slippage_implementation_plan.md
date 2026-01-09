# Slippage & Trading Costs Implementation Plan

**Date:** 2026-01-09
**Status:** IMPLEMENTED
**Principle:** Keep pricing and trading costs/slippage **separate**

## Implementation Status

All phases have been implemented:

- [x] Phase 1: Core Domain Objects (`cs-domain/src/trading_costs/`)
- [x] Phase 2: Cost Models (NoCost, FixedPerLeg, Percentage, HalfSpread, IVBased, Commission, Composite)
- [x] Phase 3: Configuration (TradingCostConfig with serde)
- [x] Phase 4: Integration (ExecutionConfig, CompositePricing.to_trading_context())
- [ ] Phase 5: Testing (unit tests implemented, integration tests pending)

---

## 1. Design Philosophy

### Separation of Concerns

```
┌─────────────────────────────────────────────────────────────────┐
│                        PRICING LAYER                            │
│  spread_pricer.rs / composite_pricer.rs                         │
│  - Pure market data → prices                                    │
│  - IV calculations                                              │
│  - Greeks                                                       │
│  - NO knowledge of costs                                        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    TRADING COSTS LAYER                          │
│  TradingCostCalculator (NEW)                                    │
│  - Slippage models                                              │
│  - Commission models                                            │
│  - Market impact models                                         │
│  - Applied AFTER pricing, BEFORE result construction            │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      EXECUTION LAYER                            │
│  execution/*_impl.rs                                            │
│  - Combines pricing + costs                                     │
│  - Constructs final results                                     │
└─────────────────────────────────────────────────────────────────┘
```

### Key Principles

1. **Pricing is pure:** `CompositePricing` returns theoretical mid-price values
2. **Costs are separate:** `TradingCostCalculator` computes costs independently
3. **Results combine both:** `to_result()` subtracts costs from P&L
4. **Configurable:** All cost models constructed from config
5. **Composable:** Multiple cost models can stack (slippage + commission + impact)

---

## 2. Core Domain Objects

### 2.1 TradingCostCalculator (Trait)

```rust
// cs-domain/src/trading_costs/calculator.rs

/// Calculates trading costs for a trade
///
/// This is the core abstraction - all cost models implement this trait.
/// Costs are computed separately from pricing and subtracted from P&L.
pub trait TradingCostCalculator: Send + Sync {
    /// Calculate the cost for entering a position
    ///
    /// # Arguments
    /// * `pricing` - The theoretical pricing (mid-price based)
    /// * `context` - Additional context (IV, spot, etc.)
    ///
    /// # Returns
    /// TradingCost with breakdown
    fn entry_cost(
        &self,
        pricing: &CompositePricing,
        context: &TradingContext,
    ) -> TradingCost;

    /// Calculate the cost for exiting a position
    fn exit_cost(
        &self,
        pricing: &CompositePricing,
        context: &TradingContext,
    ) -> TradingCost;

    /// Calculate round-trip cost (entry + exit)
    fn round_trip_cost(
        &self,
        entry_pricing: &CompositePricing,
        exit_pricing: &CompositePricing,
        context: &TradingContext,
    ) -> TradingCost {
        let entry = self.entry_cost(entry_pricing, context);
        let exit = self.exit_cost(exit_pricing, context);
        entry + exit
    }

    /// Name of this cost model (for logging/display)
    fn name(&self) -> &str;
}
```

### 2.2 TradingCost (Value Object)

```rust
// cs-domain/src/trading_costs/cost.rs

/// Represents a trading cost with full breakdown
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TradingCost {
    /// Total cost (always positive, subtracted from P&L)
    pub total: Decimal,

    /// Breakdown by component
    pub breakdown: TradingCostBreakdown,

    /// Which side this cost applies to
    pub side: TradeSide,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TradingCostBreakdown {
    /// Slippage cost (bid-ask spread)
    pub slippage: Decimal,

    /// Commission/fees
    pub commission: Decimal,

    /// Market impact (for large orders)
    pub market_impact: Decimal,

    /// Other costs
    pub other: Decimal,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum TradeSide {
    #[default]
    Entry,
    Exit,
}

impl std::ops::Add for TradingCost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            total: self.total + other.total,
            breakdown: TradingCostBreakdown {
                slippage: self.breakdown.slippage + other.breakdown.slippage,
                commission: self.breakdown.commission + other.breakdown.commission,
                market_impact: self.breakdown.market_impact + other.breakdown.market_impact,
                other: self.breakdown.other + other.breakdown.other,
            },
            side: TradeSide::Entry, // Combined
        }
    }
}
```

### 2.3 TradingContext (Context Object)

```rust
// cs-domain/src/trading_costs/context.rs

/// Context for cost calculations
///
/// Provides market data needed by cost models without coupling to pricing.
#[derive(Debug, Clone)]
pub struct TradingContext {
    /// Number of legs in the trade
    pub num_legs: usize,

    /// Number of contracts
    pub num_contracts: u32,

    /// Average IV across legs (for IV-based models)
    pub avg_iv: Option<f64>,

    /// Spot price of underlying
    pub spot_price: f64,

    /// Time of trade (for time-based models)
    pub trade_time: DateTime<Utc>,

    /// Underlying symbol
    pub symbol: String,

    /// Trade type (for type-specific costs)
    pub trade_type: TradeType,
}

#[derive(Debug, Clone, Copy)]
pub enum TradeType {
    Straddle,
    Strangle,
    CalendarSpread,
    IronButterfly,
    IronCondor,
    VerticalSpread,
    Custom,
}

impl TradingContext {
    pub fn from_pricing(
        pricing: &CompositePricing,
        symbol: &str,
        spot: f64,
        time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> Self {
        Self {
            num_legs: pricing.legs.len(),
            num_contracts: 1, // Default, can be overridden
            avg_iv: pricing.avg_iv,
            spot_price: spot,
            trade_time: time,
            symbol: symbol.to_string(),
            trade_type,
        }
    }
}
```

---

## 3. Slippage Models

### 3.1 FixedPerLegSlippage

**Model:** Fixed dollar amount per leg per contract

```rust
// cs-domain/src/trading_costs/models/fixed_per_leg.rs

/// Fixed cost per leg
///
/// Simple model: each leg costs a fixed amount to trade.
/// Good baseline for liquid options.
///
/// # Example
/// - cost_per_leg = $0.02 per share
/// - Straddle (2 legs): $0.02 × 2 × 100 = $4.00 per contract
/// - Iron Butterfly (4 legs): $0.02 × 4 × 100 = $8.00 per contract
pub struct FixedPerLegSlippage {
    /// Cost per leg per share (e.g., $0.02)
    cost_per_leg: Decimal,

    /// Contract multiplier (typically 100)
    multiplier: u32,
}

impl FixedPerLegSlippage {
    pub fn new(cost_per_leg: Decimal) -> Self {
        Self {
            cost_per_leg,
            multiplier: 100,
        }
    }

    /// Common preset: $0.01 per leg (tight markets)
    pub fn tight() -> Self {
        Self::new(dec!(0.01))
    }

    /// Common preset: $0.02 per leg (normal markets)
    pub fn normal() -> Self {
        Self::new(dec!(0.02))
    }

    /// Common preset: $0.05 per leg (wide markets / illiquid)
    pub fn wide() -> Self {
        Self::new(dec!(0.05))
    }
}

impl TradingCostCalculator for FixedPerLegSlippage {
    fn entry_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        let cost = self.cost_per_leg
            * Decimal::from(ctx.num_legs)
            * Decimal::from(self.multiplier)
            * Decimal::from(ctx.num_contracts);

        TradingCost {
            total: cost,
            breakdown: TradingCostBreakdown {
                slippage: cost,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        // Same as entry
        let mut cost = self.entry_cost(pricing, ctx);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "FixedPerLeg"
    }
}
```

### 3.2 PercentageOfPremiumSlippage

**Model:** Percentage of the premium paid/received

```rust
// cs-domain/src/trading_costs/models/percentage.rs

/// Slippage as percentage of premium
///
/// Cost scales with the size of the trade.
/// More realistic for varying premium sizes.
///
/// # Example
/// - slippage_bps = 50 (0.50%)
/// - Entry premium = $2.00 per share
/// - Slippage = $2.00 × 0.50% × 100 = $1.00 per contract
pub struct PercentageOfPremiumSlippage {
    /// Slippage in basis points (1 bp = 0.01%)
    slippage_bps: u32,

    /// Minimum cost per leg (floor)
    min_cost_per_leg: Decimal,

    /// Maximum cost per leg (cap)
    max_cost_per_leg: Option<Decimal>,

    /// Contract multiplier
    multiplier: u32,
}

impl PercentageOfPremiumSlippage {
    pub fn new(slippage_bps: u32) -> Self {
        Self {
            slippage_bps,
            min_cost_per_leg: dec!(0.01), // $0.01 minimum
            max_cost_per_leg: None,
            multiplier: 100,
        }
    }

    pub fn with_bounds(slippage_bps: u32, min: Decimal, max: Decimal) -> Self {
        Self {
            slippage_bps,
            min_cost_per_leg: min,
            max_cost_per_leg: Some(max),
            multiplier: 100,
        }
    }

    /// Preset: 25 bps (tight)
    pub fn tight() -> Self {
        Self::new(25)
    }

    /// Preset: 50 bps (normal)
    pub fn normal() -> Self {
        Self::new(50)
    }

    /// Preset: 100 bps (wide)
    pub fn wide() -> Self {
        Self::new(100)
    }

    fn calculate_leg_cost(&self, leg_price: Decimal) -> Decimal {
        let pct = Decimal::from(self.slippage_bps) / dec!(10000);
        let cost = (leg_price.abs() * pct).max(self.min_cost_per_leg);

        match self.max_cost_per_leg {
            Some(max) => cost.min(max),
            None => cost,
        }
    }
}

impl TradingCostCalculator for PercentageOfPremiumSlippage {
    fn entry_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        // Sum cost across all legs
        let leg_cost: Decimal = pricing.legs.iter()
            .map(|(leg, _)| self.calculate_leg_cost(leg.price))
            .sum();

        let total = leg_cost
            * Decimal::from(self.multiplier)
            * Decimal::from(ctx.num_contracts);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                slippage: total,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        let mut cost = self.entry_cost(pricing, ctx);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "PercentageOfPremium"
    }
}
```

### 3.3 IVBasedSlippage

**Model:** Spread widens with IV (more realistic for earnings plays)

```rust
// cs-domain/src/trading_costs/models/iv_based.rs

/// IV-based slippage model
///
/// Higher IV = wider bid-ask spreads = more slippage.
/// Particularly relevant for earnings trades where IV is elevated.
///
/// # Formula
/// spread_pct = base_spread + (iv_multiplier × IV)
/// cost = premium × spread_pct / 2  (half-spread)
///
/// # Example
/// - base_spread = 2% (0.02)
/// - iv_multiplier = 0.05
/// - IV = 80% (0.80)
/// - spread_pct = 0.02 + (0.05 × 0.80) = 0.06 (6%)
/// - premium = $3.00
/// - half_spread_cost = $3.00 × 6% / 2 = $0.09 per share
pub struct IVBasedSlippage {
    /// Base spread percentage (applied even at 0 IV)
    base_spread_pct: f64,

    /// How much spread widens per unit of IV
    iv_multiplier: f64,

    /// Maximum spread percentage (cap)
    max_spread_pct: f64,

    /// Contract multiplier
    multiplier: u32,
}

impl IVBasedSlippage {
    pub fn new(base_spread_pct: f64, iv_multiplier: f64) -> Self {
        Self {
            base_spread_pct,
            iv_multiplier,
            max_spread_pct: 0.20, // 20% max spread
            multiplier: 100,
        }
    }

    /// Preset: Conservative (tighter spreads)
    pub fn conservative() -> Self {
        Self::new(0.01, 0.02) // 1% base + 2% per IV unit
    }

    /// Preset: Moderate
    pub fn moderate() -> Self {
        Self::new(0.02, 0.05) // 2% base + 5% per IV unit
    }

    /// Preset: Aggressive (wider spreads, illiquid)
    pub fn aggressive() -> Self {
        Self::new(0.03, 0.10) // 3% base + 10% per IV unit
    }

    fn spread_percentage(&self, iv: f64) -> f64 {
        let spread = self.base_spread_pct + (self.iv_multiplier * iv);
        spread.min(self.max_spread_pct)
    }

    fn half_spread_cost(&self, price: Decimal, iv: f64) -> Decimal {
        let spread_pct = self.spread_percentage(iv);
        let half_spread = spread_pct / 2.0;
        price.abs() * Decimal::try_from(half_spread).unwrap_or(Decimal::ZERO)
    }
}

impl TradingCostCalculator for IVBasedSlippage {
    fn entry_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        let iv = ctx.avg_iv.unwrap_or(0.30); // Default 30% IV

        // Calculate per-leg costs using leg-specific IV if available
        let leg_cost: Decimal = pricing.legs.iter()
            .map(|(leg, _)| {
                let leg_iv = leg.iv.unwrap_or(iv);
                self.half_spread_cost(leg.price, leg_iv)
            })
            .sum();

        let total = leg_cost
            * Decimal::from(self.multiplier)
            * Decimal::from(ctx.num_contracts);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                slippage: total,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        let mut cost = self.entry_cost(pricing, ctx);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "IVBased"
    }
}
```

### 3.4 HalfSpreadSlippage

**Model:** Most realistic - uses actual bid-ask spread percentage

```rust
// cs-domain/src/trading_costs/models/half_spread.rs

/// Half-spread model (most realistic)
///
/// Assumes you cross the spread: buy at ask, sell at bid.
/// Cost = half the bid-ask spread on each side.
///
/// Since historical data typically has mid prices only,
/// we estimate the spread based on configurable percentages.
///
/// # Formula
/// Entry: pay mid + half_spread (buying at ask)
/// Exit: receive mid - half_spread (selling at bid)
/// Round-trip cost = full spread
///
/// # Example
/// - spread_pct = 4% (bid $2.88, ask $3.12, mid $3.00)
/// - Entry: pay $3.00 + $0.06 = $3.06
/// - Exit: receive $3.00 - $0.06 = $2.94
/// - Round-trip slippage: $0.12 per share = $12 per contract
pub struct HalfSpreadSlippage {
    /// Assumed bid-ask spread as percentage of mid price
    spread_pct: f64,

    /// Minimum spread in dollars (floor)
    min_spread: Decimal,

    /// Contract multiplier
    multiplier: u32,
}

impl HalfSpreadSlippage {
    pub fn new(spread_pct: f64) -> Self {
        Self {
            spread_pct,
            min_spread: dec!(0.01), // $0.01 minimum spread
            multiplier: 100,
        }
    }

    /// Preset: Tight spread (2%)
    pub fn tight() -> Self {
        Self::new(0.02)
    }

    /// Preset: Normal spread (4%)
    pub fn normal() -> Self {
        Self::new(0.04)
    }

    /// Preset: Wide spread (8%)
    pub fn wide() -> Self {
        Self::new(0.08)
    }

    /// Preset: Very wide (12%) - illiquid options
    pub fn illiquid() -> Self {
        Self::new(0.12)
    }

    fn half_spread(&self, price: Decimal) -> Decimal {
        let full_spread = price.abs() * Decimal::try_from(self.spread_pct).unwrap_or(Decimal::ZERO);
        let spread = full_spread.max(self.min_spread);
        spread / dec!(2)
    }
}

impl TradingCostCalculator for HalfSpreadSlippage {
    fn entry_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        // On entry, we cross the spread (pay the half-spread)
        let leg_cost: Decimal = pricing.legs.iter()
            .map(|(leg, _)| self.half_spread(leg.price))
            .sum();

        let total = leg_cost
            * Decimal::from(self.multiplier)
            * Decimal::from(ctx.num_contracts);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                slippage: total,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        // On exit, we also cross the spread
        let mut cost = self.entry_cost(pricing, ctx);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "HalfSpread"
    }
}
```

### 3.5 CommissionModel

**Model:** Broker commissions (separate from slippage)

```rust
// cs-domain/src/trading_costs/models/commission.rs

/// Commission model
///
/// Broker fees per contract or per trade.
///
/// # Common Structures
/// - Per contract: $0.65 per contract
/// - Per contract with cap: $0.65 per contract, max $10 per leg
/// - Tiered: Lower rates for higher volume
pub struct CommissionModel {
    /// Commission per contract
    per_contract: Decimal,

    /// Maximum commission per leg (cap)
    max_per_leg: Option<Decimal>,

    /// Minimum commission per order (floor)
    min_per_order: Decimal,
}

impl CommissionModel {
    pub fn new(per_contract: Decimal) -> Self {
        Self {
            per_contract,
            max_per_leg: None,
            min_per_order: Decimal::ZERO,
        }
    }

    /// Interactive Brokers-like pricing
    pub fn ibkr() -> Self {
        Self {
            per_contract: dec!(0.65),
            max_per_leg: Some(dec!(10.00)),
            min_per_order: dec!(1.00),
        }
    }

    /// Tastytrade-like pricing
    pub fn tastytrade() -> Self {
        Self {
            per_contract: dec!(1.00), // $1 to open, $0 to close
            max_per_leg: Some(dec!(10.00)),
            min_per_order: Decimal::ZERO,
        }
    }

    /// Zero commission (Robinhood, etc.)
    pub fn zero() -> Self {
        Self::new(Decimal::ZERO)
    }
}

impl TradingCostCalculator for CommissionModel {
    fn entry_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        let contracts = Decimal::from(ctx.num_contracts);
        let legs = ctx.num_legs;

        let per_leg = (self.per_contract * contracts)
            .min(self.max_per_leg.unwrap_or(Decimal::MAX));

        let total = (per_leg * Decimal::from(legs))
            .max(self.min_per_order);

        TradingCost {
            total,
            breakdown: TradingCostBreakdown {
                commission: total,
                ..Default::default()
            },
            side: TradeSide::Entry,
        }
    }

    fn exit_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        let mut cost = self.entry_cost(pricing, ctx);
        cost.side = TradeSide::Exit;
        cost
    }

    fn name(&self) -> &str {
        "Commission"
    }
}
```

### 3.6 CompositeCostCalculator

**Model:** Combines multiple cost models

```rust
// cs-domain/src/trading_costs/models/composite.rs

/// Combines multiple cost calculators
///
/// Allows stacking slippage + commission + market impact.
///
/// # Example
/// ```rust
/// let calculator = CompositeCostCalculator::new()
///     .with(HalfSpreadSlippage::normal())
///     .with(CommissionModel::ibkr());
/// ```
pub struct CompositeCostCalculator {
    calculators: Vec<Box<dyn TradingCostCalculator>>,
}

impl CompositeCostCalculator {
    pub fn new() -> Self {
        Self { calculators: Vec::new() }
    }

    pub fn with<C: TradingCostCalculator + 'static>(mut self, calc: C) -> Self {
        self.calculators.push(Box::new(calc));
        self
    }

    /// Common preset: Slippage + Commission
    pub fn realistic() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::normal())
            .with(CommissionModel::ibkr())
    }

    /// Slippage only (no commission)
    pub fn slippage_only() -> Self {
        Self::new()
            .with(HalfSpreadSlippage::normal())
    }
}

impl TradingCostCalculator for CompositeCostCalculator {
    fn entry_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        self.calculators.iter()
            .map(|c| c.entry_cost(pricing, ctx))
            .fold(TradingCost::default(), |acc, cost| acc + cost)
    }

    fn exit_cost(&self, pricing: &CompositePricing, ctx: &TradingContext) -> TradingCost {
        self.calculators.iter()
            .map(|c| c.exit_cost(pricing, ctx))
            .fold(TradingCost::default(), |acc, cost| acc + cost)
    }

    fn name(&self) -> &str {
        "Composite"
    }
}
```

### 3.7 NoCost (Null Object)

```rust
// cs-domain/src/trading_costs/models/no_cost.rs

/// No trading costs (null object pattern)
///
/// Use when you want to disable costs without changing code.
pub struct NoCost;

impl TradingCostCalculator for NoCost {
    fn entry_cost(&self, _: &CompositePricing, _: &TradingContext) -> TradingCost {
        TradingCost::default()
    }

    fn exit_cost(&self, _: &CompositePricing, _: &TradingContext) -> TradingCost {
        TradingCost::default()
    }

    fn name(&self) -> &str {
        "NoCost"
    }
}
```

---

## 4. Configuration

### 4.1 TradingCostConfig

```rust
// cs-domain/src/trading_costs/config.rs

/// Configuration for trading costs
///
/// Deserializable from TOML/JSON for easy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum TradingCostConfig {
    /// No costs
    None,

    /// Fixed cost per leg
    FixedPerLeg {
        cost_per_leg: Decimal,
    },

    /// Percentage of premium
    Percentage {
        slippage_bps: u32,
        #[serde(default)]
        min_cost_per_leg: Option<Decimal>,
        #[serde(default)]
        max_cost_per_leg: Option<Decimal>,
    },

    /// IV-based spread
    IvBased {
        base_spread_pct: f64,
        iv_multiplier: f64,
        #[serde(default = "default_max_spread")]
        max_spread_pct: f64,
    },

    /// Half-spread model
    HalfSpread {
        spread_pct: f64,
    },

    /// Commission only
    Commission {
        per_contract: Decimal,
        #[serde(default)]
        max_per_leg: Option<Decimal>,
    },

    /// Composite (slippage + commission)
    Composite {
        slippage: Box<TradingCostConfig>,
        commission: Box<TradingCostConfig>,
    },

    /// Preset configurations
    Preset {
        name: CostPreset,
    },
}

fn default_max_spread() -> f64 { 0.20 }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostPreset {
    /// Tight markets, low costs
    Tight,
    /// Normal market conditions
    Normal,
    /// Wide spreads, illiquid
    Wide,
    /// Realistic (slippage + commission)
    Realistic,
    /// IBKR-like costs
    Ibkr,
}

impl TradingCostConfig {
    /// Build the calculator from config
    pub fn build(&self) -> Box<dyn TradingCostCalculator> {
        match self {
            Self::None => Box::new(NoCost),

            Self::FixedPerLeg { cost_per_leg } =>
                Box::new(FixedPerLegSlippage::new(*cost_per_leg)),

            Self::Percentage { slippage_bps, min_cost_per_leg, max_cost_per_leg } => {
                let mut model = PercentageOfPremiumSlippage::new(*slippage_bps);
                if let Some(min) = min_cost_per_leg {
                    model.min_cost_per_leg = *min;
                }
                model.max_cost_per_leg = *max_cost_per_leg;
                Box::new(model)
            }

            Self::IvBased { base_spread_pct, iv_multiplier, max_spread_pct } => {
                let mut model = IVBasedSlippage::new(*base_spread_pct, *iv_multiplier);
                model.max_spread_pct = *max_spread_pct;
                Box::new(model)
            }

            Self::HalfSpread { spread_pct } =>
                Box::new(HalfSpreadSlippage::new(*spread_pct)),

            Self::Commission { per_contract, max_per_leg } => {
                let mut model = CommissionModel::new(*per_contract);
                model.max_per_leg = *max_per_leg;
                Box::new(model)
            }

            Self::Composite { slippage, commission } => {
                Box::new(CompositeCostCalculator::new()
                    .with_boxed(slippage.build())
                    .with_boxed(commission.build()))
            }

            Self::Preset { name } => name.build(),
        }
    }
}

impl CostPreset {
    pub fn build(&self) -> Box<dyn TradingCostCalculator> {
        match self {
            Self::Tight => Box::new(HalfSpreadSlippage::tight()),
            Self::Normal => Box::new(HalfSpreadSlippage::normal()),
            Self::Wide => Box::new(HalfSpreadSlippage::wide()),
            Self::Realistic => Box::new(CompositeCostCalculator::realistic()),
            Self::Ibkr => Box::new(CompositeCostCalculator::new()
                .with(HalfSpreadSlippage::normal())
                .with(CommissionModel::ibkr())),
        }
    }
}

impl Default for TradingCostConfig {
    fn default() -> Self {
        Self::Preset { name: CostPreset::Normal }
    }
}
```

### 4.2 TOML Config Examples

```toml
# Example 1: Simple preset
[trading_costs]
model = "preset"
name = "normal"

# Example 2: Fixed per leg
[trading_costs]
model = "fixed_per_leg"
cost_per_leg = 0.02

# Example 3: Percentage-based
[trading_costs]
model = "percentage"
slippage_bps = 50
min_cost_per_leg = 0.01
max_cost_per_leg = 0.10

# Example 4: IV-based (best for earnings)
[trading_costs]
model = "iv_based"
base_spread_pct = 0.02
iv_multiplier = 0.05
max_spread_pct = 0.15

# Example 5: Half-spread
[trading_costs]
model = "half_spread"
spread_pct = 0.04

# Example 6: Composite (slippage + commission)
[trading_costs]
model = "composite"

[trading_costs.slippage]
model = "half_spread"
spread_pct = 0.04

[trading_costs.commission]
model = "commission"
per_contract = 0.65
max_per_leg = 10.00

# Example 7: No costs (for comparison)
[trading_costs]
model = "none"
```

---

## 5. Module Structure

```
cs-domain/src/
├── trading_costs/
│   ├── mod.rs              # Module root, re-exports
│   ├── calculator.rs       # TradingCostCalculator trait
│   ├── cost.rs             # TradingCost value object
│   ├── context.rs          # TradingContext
│   ├── config.rs           # TradingCostConfig, CostPreset
│   └── models/
│       ├── mod.rs          # Model re-exports
│       ├── no_cost.rs      # NoCost (null object)
│       ├── fixed_per_leg.rs
│       ├── percentage.rs
│       ├── iv_based.rs
│       ├── half_spread.rs
│       ├── commission.rs
│       └── composite.rs
```

---

## 6. Integration Points

### 6.1 ExecutionConfig Update

```rust
// cs-backtest/src/execution/types.rs

pub struct ExecutionConfig {
    pub max_entry_iv: Option<f64>,
    pub min_entry_cost: Decimal,
    pub min_credit: Option<Decimal>,

    // NEW: Trading costs configuration
    pub trading_costs: TradingCostConfig,
}

impl ExecutionConfig {
    pub fn cost_calculator(&self) -> Box<dyn TradingCostCalculator> {
        self.trading_costs.build()
    }
}
```

### 6.2 Result Struct Update

```rust
// cs-domain/src/entities.rs

pub struct StraddleResult {
    // ... existing fields ...

    /// Trading costs (slippage + commission)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trading_cost: Option<TradingCost>,
}
```

### 6.3 Execution Flow

```rust
// cs-backtest/src/execution/straddle_impl.rs

fn to_result(
    &self,
    entry_pricing: CompositePricing,
    exit_pricing: CompositePricing,
    output: &SimulationOutput,
    event: Option<&EarningsEvent>,
    cost_calculator: &dyn TradingCostCalculator,  // NEW
) -> StraddleResult {
    // Build context
    let ctx = TradingContext::from_pricing(
        &entry_pricing,
        self.symbol(),
        output.entry_spot,
        output.entry_time,
        TradeType::Straddle,
    );

    // Calculate costs (SEPARATE from pricing)
    let entry_cost = cost_calculator.entry_cost(&entry_pricing, &ctx);
    let exit_cost = cost_calculator.exit_cost(&exit_pricing, &ctx);
    let total_trading_cost = entry_cost + exit_cost;

    // P&L calculation (pricing is clean, costs subtracted)
    let pnl_per_share = exit_pricing.net_cost - entry_pricing.net_cost;
    let gross_pnl = pnl_per_share * Decimal::from(CONTRACT_MULTIPLIER);
    let net_pnl = gross_pnl - total_trading_cost.total;

    StraddleResult {
        // ... existing fields ...
        pnl: net_pnl,  // Net of costs
        trading_cost: Some(total_trading_cost),
    }
}
```

---

## 7. Implementation Order

### Phase 1: Core Domain Objects
1. Create `cs-domain/src/trading_costs/` module structure
2. Implement `TradingCost` value object
3. Implement `TradingContext`
4. Implement `TradingCostCalculator` trait

### Phase 2: Cost Models
5. Implement `NoCost` (null object)
6. Implement `FixedPerLegSlippage`
7. Implement `PercentageOfPremiumSlippage`
8. Implement `HalfSpreadSlippage`
9. Implement `IVBasedSlippage`
10. Implement `CommissionModel`
11. Implement `CompositeCostCalculator`

### Phase 3: Configuration
12. Implement `TradingCostConfig` with serde
13. Implement `CostPreset` enum
14. Add to `ExecutionConfig`
15. Update TOML parsing

### Phase 4: Integration
16. Add `trading_cost` field to all result structs
17. Update all `to_result()` methods (8 trade types)
18. Update CLI to display trading costs
19. Update accounting module to include costs

### Phase 5: Testing
20. Unit tests for each cost model
21. Integration tests with sample trades
22. Verify P&L calculations with costs

---

## 8. Example Output

After implementation, backtest output would show:

```
Results:
+-------------------------+-------------------+
| Metric                  | Value             |
+-------------------------+-------------------+
| Total P&L (gross)       | $68.00            |
| Trading Costs           | -$12.90           |
|   Slippage              | -$10.32           |
|   Commission            | -$2.58            |
| Total P&L (net)         | $55.10            |
+-------------------------+-------------------+

Capital-Weighted Metrics:
+-------------------------+-------------------+
| Return on Capital       | 42.7%             |
| (before costs: 52.7%)   |                   |
+-------------------------+-------------------+
```

---

## 9. Summary

| Component | Purpose |
|-----------|---------|
| `TradingCostCalculator` | Trait for all cost models |
| `TradingCost` | Value object with breakdown |
| `TradingContext` | Market context for calculations |
| `FixedPerLegSlippage` | Simple fixed cost per leg |
| `PercentageOfPremiumSlippage` | Cost scales with premium |
| `IVBasedSlippage` | Spread widens with IV |
| `HalfSpreadSlippage` | Realistic bid-ask model |
| `CommissionModel` | Broker fees |
| `CompositeCostCalculator` | Combines multiple models |
| `TradingCostConfig` | Serde-compatible config |

**Key Principle:** Pricing stays pure. Costs are calculated separately and subtracted at the end.
