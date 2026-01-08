# Multi-Leg Volatility Strategies: Unified Architecture

**Date**: 2026-01-08
**Status**: Design Phase
**Scope**: Implement Strangle, Condor, Iron Condor, Butterfly with shared refactored code

---

## Objective

Design and implement a unified architecture for multi-leg volatility strategies that:
1. Eliminates code duplication
2. Supports both long and short versions uniformly
3. Uses common strike positioning patterns (delta, moneyness, symmetric)
4. Makes adding new strategies simple and maintainable

---

## Strategy Analysis

### Strategy Structures

| Strategy | Legs | Structure | Greeks Profile |
|----------|------|-----------|-----------------|
| **Strangle (Short)** | 2 | Short OTM Call + Short OTM Put | +Theta, -Vega |
| **Strangle (Long)** | 2 | Long OTM Call + Long OTM Put | -Theta, +Vega |
| **Butterfly (Short)** | 4 | Short 2x ATM Straddle + Long OTM Wings | +Theta, -Vega |
| **Butterfly (Long)** | 4 | Long 2x ATM Straddle + Short OTM Wings | -Theta, +Vega |
| **Iron Butterfly (Short)** | 4 | Short ATM Straddle + Long OTM Wings | +Theta, -Vega |
| **Iron Butterfly (Long)** | 4 | Long ATM Straddle + Short OTM Wings | -Theta, +Vega |
| **Condor (Short)** | 4 | Short Near Straddle + Long Far Wings | +Theta, -Vega |
| **Condor (Long)** | 4 | Long Near Straddle + Short Far Wings | -Theta, +Vega |
| **Iron Condor (Short)** | 4 | Short Near Spread + Long Far Wings | +Theta, -Vega |
| **Iron Condor (Long)** | 4 | Long Near Spread + Short Far Wings | -Theta, +Vega |

### Common Patterns Identified

#### 1. Symmetry
- **Symmetric**: Strangle, Butterfly, Iron Butterfly, Condor, Iron Condor (call and put at equal distance from center)
- Support: Delta-based (0.25 delta symmetry) and moneyness-based (10% OTM symmetry)

#### 2. Position Structure
- **2-Leg Strategies**: Strangle (simple, price-directional bias)
- **4-Leg Strategies**: Butterfly, Condor, Iron Butterfly, Iron Condor (complex, non-directional)

#### 3. Center Strike Positioning
- **ATM-based**: IronButterfly, Butterfly, Strangle - use spot price as anchor
- **Spread-based**: Condor, IronCondor - create spreads at different OTM levels

#### 4. Wing Configuration
- **Fixed Width**: Single distance from center (Strangle uses OTM distance)
- **Double Width**: Two different distances for inner/outer strikes (Condor, Iron Condor)
- **Multiplicity**: How many legs at center (1x for Strangle, 2x for Butterfly, 2x for Condor)

#### 5. Direction Handling
- All strategies have Long and Short versions
- Direction flips all leg positions uniformly: Long ↔ Short call/put

---

## Solution Architecture

### Layer 1: Domain Value Objects

**File**: `cs-domain/src/value_objects/volatility_strategies.rs`

```rust
/// Wing configuration for symmetrical strategies
#[derive(Debug, Clone)]
pub struct SymmetricWingConfig {
    pub selection_mode: WingSelectionMode,  // Delta or Moneyness
    pub symmetric: bool,
    pub spread_type: SpreadType,  // Simple, Double, etc.
}

/// Spread type defines the wing structure
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpreadType {
    /// Single distance from center (e.g., Strangle: OTM distance)
    Simple { distance_from_center: DistanceSpec },

    /// Two distances for inner/outer strikes (e.g., Condor: near and far)
    Double {
        near_distance: DistanceSpec,
        far_distance: DistanceSpec,
    },
}

/// How distance is specified (delta or moneyness percent)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceSpec {
    Delta(f64),
    Moneyness(f64),
}

/// Center configuration for the strategy
#[derive(Debug, Clone, Copy)]
pub struct CenterConfig {
    /// Number of legs at center (1 for Strangle, 2 for Butterfly)
    pub multiplicity: u32,
    /// Whether center is a straddle (both call and put) or spread
    pub is_straddle: bool,
}

/// Multi-leg strategy configuration (unified)
#[derive(Debug, Clone)]
pub struct MultiLegStrategyConfig {
    pub strategy_type: MultiLegStrategyType,
    pub center: CenterConfig,
    pub wings: SymmetricWingConfig,
    pub direction: TradeDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiLegStrategyType {
    Strangle,
    Butterfly,
    IronButterfly,
    Condor,
    IronCondor,
}
```

### Layer 2: Strike Selection

**File**: `cs-domain/src/strike_selection/multi_leg.rs`

```rust
/// Common trait for multi-leg strike selection
pub trait MultiLegSelector: Send + Sync {
    fn select_multi_leg(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        config: &MultiLegStrategyConfig,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<MultiLegStrikeSelection, SelectionError>;
}

/// Selected strikes for a multi-leg strategy
#[derive(Debug, Clone)]
pub struct MultiLegStrikeSelection {
    pub center_strikes: Vec<Strike>,  // 1 for Strangle, 2 for Butterfly
    pub near_strikes: Option<Vec<Strike>>,  // Inner wings (for Condor)
    pub far_strikes: Option<Vec<Strike>>,   // Outer wings
    pub expiration: NaiveDate,
}

/// Unified strike selection implementation
pub struct SymmetricMultiLegSelector {
    pub risk_free_rate: f64,
}

impl MultiLegSelector for SymmetricMultiLegSelector {
    fn select_multi_leg(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        config: &MultiLegStrategyConfig,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<MultiLegStrikeSelection, SelectionError> {
        // 1. Select expiration by DTE
        // 2. Find center strikes (ATM or spread)
        // 3. Select wings based on SpreadType (Simple or Double)
        // 4. Validate symmetric constraint
        // 5. Snap to available strikes
        // 6. Return selection
    }
}
```

### Layer 3: Multi-Leg Entities

**File**: `cs-domain/src/entities/multi_leg_trades.rs`

```rust
/// Generic multi-leg trade result
#[derive(Debug, Clone)]
pub struct MultiLegTradeResult {
    pub symbol: String,
    pub strategy: MultiLegStrategyType,
    pub direction: TradeDirection,

    // Legs
    pub legs: Vec<OptionLeg>,
    pub positions: Vec<LegPosition>,  // Long or Short

    // Pricing
    pub entry_cost: Decimal,
    pub exit_cost: Decimal,
    pub pnl: Decimal,

    // Greeks
    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegPosition {
    Long,
    Short,
}

/// Trait for all multi-leg strategies
pub trait MultiLegTrade: Sized + Send + Sync {
    type Result: Send + Sync;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<Self, TradeConstructionError>;

    fn legs(&self) -> &[OptionLeg];
    fn positions(&self) -> &[LegPosition];
    fn expiration(&self) -> NaiveDate;
    fn center_strike(&self) -> Decimal;
}
```

### Layer 4: TradeFactory Extension

**File**: `cs-domain/src/ports/trade_factory.rs`

```rust
/// New factory method for multi-leg strategies
#[async_trait]
pub trait TradeFactory: Send + Sync {
    // Existing methods...

    /// Create any multi-leg strategy with unified configuration
    async fn create_multi_leg_trade(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<Box<dyn std::any::Any + Send>, TradeFactoryError>;
}
```

### Layer 5: Implementation

**File**: `cs-backtest/src/trade_factory_multi_leg.rs`

```rust
impl TradeFactory for DefaultTradeFactory {
    async fn create_multi_leg_trade(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &MultiLegStrategyConfig,
    ) -> Result<Box<dyn std::any::Any + Send>, TradeFactoryError> {
        // 1. Get option chain
        // 2. Build IV surface
        // 3. Use SymmetricMultiLegSelector
        // 4. Based on config.strategy_type, create the appropriate trade:
        //    - Strangle::new(...)
        //    - Butterfly::new(...)
        //    - IronButterfly::new(...)
        //    - Condor::new(...)
        //    - IronCondor::new(...)
        // 5. Return as boxed trait object
    }
}
```

---

## Strategy Specifications

### 1. Strangle

```
SHORT STRANGLE (Sell Call + Sell Put):
- Sell OTM Call at +delta distance
- Sell OTM Put at -delta distance
- Profit: If stock stays between strikes
- Greeks: +Theta, -Vega

LONG STRANGLE (Buy Call + Buy Put):
- Buy OTM Call at +delta distance
- Buy OTM Put at -delta distance
- Profit: If stock moves beyond strikes
- Greeks: -Theta, +Vega
```

**Implementation**:
- CenterConfig: { multiplicity: 1, is_straddle: false }
- SpreadType: Simple { distance: 0.25 delta or 10% moneyness }
- Multiplicity controls leg count (1 call + 1 put)

### 2. Butterfly

```
SHORT BUTTERFLY:
- Sell 2x ATM Straddle (short call, short put)
- Buy Long Call (upper wing)
- Buy Long Put (lower wing)
- Max profit: At center strike
- Max loss: Limited to wing width

LONG BUTTERFLY:
- Buy 2x ATM Straddle (long call, long put)
- Sell Long Call (upper wing)
- Sell Long Put (lower wing)
- Inverted P&L
```

**Implementation**:
- CenterConfig: { multiplicity: 2, is_straddle: true }
- SpreadType: Simple { distance: 0.25 delta or 10% moneyness }

### 3. Iron Butterfly

```
SHORT IRON BUTTERFLY:
- Sell ATM Straddle (short call, short put)
- Buy OTM Call (upper wing)
- Buy OTM Put (lower wing)
- Same as Butterfly but with one leg at center

LONG IRON BUTTERFLY:
- Buy ATM Straddle (long call, long put)
- Sell OTM Call (upper wing)
- Sell OTM Put (lower wing)
```

**Implementation**:
- CenterConfig: { multiplicity: 1, is_straddle: true }
- SpreadType: Simple { distance: 0.25 delta or 10% moneyness }

### 4. Condor

```
SHORT CONDOR:
- Sell near ATM Straddle (short call, short put)
- Buy far OTM Call (upper wing)
- Buy far OTM Put (lower wing)
- Larger wing width than Butterfly for wider profit zone

LONG CONDOR:
- Buy near ATM Straddle (long call, long put)
- Sell far OTM Call (upper wing)
- Sell far OTM Put (lower wing)
```

**Implementation**:
- CenterConfig: { multiplicity: 1, is_straddle: true }
- SpreadType: Double { near: 0.25 delta, far: 0.35 delta }

### 5. Iron Condor

```
SHORT IRON CONDOR:
- Sell near Call (0.20 delta)
- Sell near Put (0.20 delta)
- Buy far Call (0.10 delta)
- Buy far Put (0.10 delta)
- Maximum defined risk, maximum defined profit
- Trades between inner and outer strikes

LONG IRON CONDOR:
- Buy near Call (0.20 delta)
- Buy near Put (0.20 delta)
- Sell far Call (0.10 delta)
- Sell far Put (0.10 delta)
```

**Implementation**:
- CenterConfig: { multiplicity: 2, is_straddle: false }
- SpreadType: Double { near: 0.20 delta, far: 0.10 delta }

---

## CLI Configuration

### Unified Format

```bash
# Strangle: 25-delta OTM, short direction
./cs campaign \
  --strategy volatility \
  --vol-strategy strangle \
  --vol-config delta:0.25 \
  --direction short

# Iron Condor: 20-delta near, 10-delta far, long direction
./cs campaign \
  --strategy volatility \
  --vol-strategy iron-condor \
  --vol-config delta:0.20,0.10 \
  --direction long

# Butterfly: 10% moneyness, short direction
./cs campaign \
  --strategy volatility \
  --vol-strategy butterfly \
  --vol-config moneyness:0.10 \
  --direction short
```

---

## Implementation Phases

### Phase 1: Create shared infrastructure
1. Define MultiLegStrategyConfig, SpreadType, DistanceSpec
2. Implement SymmetricMultiLegSelector
3. Create MultiLegTrade trait

### Phase 2: Implement strategies
1. Refactor IronButterfly to use new trait
2. Implement Strangle, Butterfly
3. Implement Condor, IronCondor

### Phase 3: Factory and CLI integration
1. Extend TradeFactory for multi-leg creation
2. Implement DefaultTradeFactory::create_multi_leg_trade
3. Add CLI flags and parsing
4. Update campaign executor

### Phase 4: Testing and validation
1. Unit tests for each strategy
2. Long vs Short direction tests
3. Delta and moneyness selection tests
4. Integration tests with SessionExecutor

---

## Code Reuse Opportunities

| Component | Reuse Pattern |
|-----------|---------------|
| Strike Selection | Single `SymmetricMultiLegSelector` for all symmetric strategies |
| Expiration Selection | Reuse from existing code |
| Direction Inversion | Flip all positions uniformly |
| Wing Validation | Symmetric constraint check for all |
| Greeks Calculation | Aggregate from individual legs |
| Result Extraction | Generic `MultiLegTradeResult` |
| CLI Parsing | Unified `MultiLegStrategyConfig` parser |

---

## Advantages of This Architecture

✅ **Minimal Duplication**: Single selector handles all strategies
✅ **Easy Extension**: Adding new strategies = defining SpreadType
✅ **Uniform Long/Short**: Direction handled once in position inversion
✅ **Flexible Positioning**: Delta and moneyness both supported
✅ **Clean Contracts**: Clear trait boundaries
✅ **Type Safety**: Compile-time validation of configurations

---

## Risk Assessment

**Low Risk**:
- Refactoring existing IronButterfly to new trait
- New strategies follow same pattern

**Medium Risk**:
- Greeks aggregation correctness
- Strike snapping for Double spreads

**Mitigation**:
- Comprehensive unit tests
- Greeks validation against market data
- Gradual phasing of implementation

