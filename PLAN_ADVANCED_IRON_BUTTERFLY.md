# Advanced IronButterfly Wing Positioning & Direction Flag

**Date**: 2026-01-08
**Status**: Planning Phase - Ready for Implementation
**Scope**: Enhanced IronButterfly strike selection + Long/Short directional flag

---

## Objective

Enhance the options trading backtest system with:

1. **Advanced Wing Positioning**: Support both **delta-based** and **moneyness-based** wing selection with symmetric constraints
2. **Directional Flag**: Add `--direction long/short` CLI flag for uniform position inversion across all strategies
3. **Strategy Variants**: Support both long and short versions of multi-leg strategies (IronButterfly, CalendarSpread, Straddle, CalendarStraddle)

---

## Current State

- **IronButterfly**: Only short version implemented (short ATM straddle + long OTM wings)
- **Wings**: Fixed $10 width, no dynamic positioning based on delta or moneyness
- **Direction**: No flag to switch long/short variants
- **Limitation**: All strategies use fixed strike offsets, not market-aware positioning

---

## User Requirements

✅ **Wing Positioning**: Support both delta-based AND moneyness-based modes (user can choose)
✅ **Symmetry**: Equal width on both sides (center ± fixed_amount)
✅ **Direction**: `--direction long/short` flag applies uniformly across all strategies

---

## Solution Architecture

### Part 1: Advanced Wing Positioning for IronButterfly

#### Wing Selection Modes

**Mode 1: Delta-Based Selection**

Uses the DeltaVolSurface to map delta → strike. This is market-standard for derivatives.

```
Example: 25-delta OTM wings
  - Upper wing (call): find strike with 0.25 call delta
  - Lower wing (put): find strike with -0.25 put delta
  - Symmetric: |upper_delta| = |lower_delta|

Key advantage: Adapts to IV skew and changes in spot price
```

**Files involved**:
- `cs-analytics/src/delta_surface.rs` - provides `delta_to_strike()` conversion
- `cs-analytics/src/black_scholes.rs` - underlying delta computation
- `cs-analytics/src/vol_slice.rs` - single expiration delta-IV relationships

**Mode 2: Moneyness-Based Selection**

Uses relative strike/spot ratio (moneyness). Simple and intuitive.

```
Example: 10% OTM symmetric wings
  - Upper wing: moneyness = 1.10 (strike = spot * 1.10)
  - Lower wing: moneyness = 0.90 (strike = spot * 0.90)
  - Symmetric: equal % above and below spot

Key advantage: Simple, intuitive, consistent across spot moves
```

**Files involved**:
- `cs-analytics/src/iv_surface.rs` - moneyness calculations
- Strike interpolation by moneyness targets

#### Configuration Structure

```rust
pub enum WingSelectionMode {
    Delta {
        wing_delta: f64,  // e.g., 0.25 for 25-delta
    },
    Moneyness {
        wing_percent: f64,  // e.g., 0.10 for 10% OTM
    },
}

pub struct IronButterflyConfig {
    pub wing_mode: WingSelectionMode,
    pub symmetric: bool,  // Always true for now
}
```

#### New Strike Selection Strategy

**DeltaSymmetricIronButterflyStrategy** - replaces fixed-width logic

**Algorithm for Delta Mode**:
```
1. Build DeltaVolSurface from IVSurface
2. Select expiration by DTE range
3. Find ATM strike (center)
4. Target deltas:
   - Upper call: +wing_delta (e.g., 0.25)
   - Lower put: -wing_delta (e.g., -0.25)
5. Convert deltas → strikes:
   - upper_strike = delta_surface.delta_to_strike(+0.25, expiration, is_call=true)
   - lower_strike = delta_surface.delta_to_strike(-0.25, expiration, is_call=false)
6. Validate symmetric constraint:
   - upper_strike - center ≈ center - lower_strike (within tolerance)
7. Snap to available strikes if needed
8. Build all 4 legs with validation
```

**Algorithm for Moneyness Mode**:
```
1. Select expiration by DTE range
2. Find ATM strike = current spot price
3. Calculate moneyness targets:
   - Upper: moneyness = 1 + wing_percent (e.g., 1.10)
   - Lower: moneyness = 1 - wing_percent (e.g., 0.90)
4. Find strikes at target moneyness:
   - upper_strike = spot * (1 + wing_percent)
   - lower_strike = spot * (1 - wing_percent)
5. Snap to available strikes (round up upper, down lower)
6. Validate symmetric constraint
7. Build 4 legs
```

---

### Part 2: Direction Flag (Long vs Short Variants)

#### Domain Architecture

**New enum**: TradeDirection
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeDirection {
    Long,
    Short,
}
```

#### Iron Butterfly Variants

| Variant | Legs | Greeks | Use Case |
|---------|------|--------|----------|
| **Short** (current) | Short ATM straddle + Long OTM wings | Theta+, Vega- | Profit from IV drop or decay |
| **Long** (new) | Long ATM straddle + Short OTM wings | Theta-, Vega+ | Profit from IV rise |

#### Implementation Strategy

**Option A (Recommended)**: Extend IronButterfly struct
```rust
pub struct IronButterfly {
    pub short_call: OptionLeg,
    pub short_put: OptionLeg,
    pub long_call: OptionLeg,
    pub long_put: OptionLeg,
    pub direction: TradeDirection,  // NEW
}
```

Pros:
- Single struct, easy to extend
- Minimal disruption to existing code
- Legs keep their names (semantic clarity)

Cons:
- Naming can be confusing (short_call means "leg that is short" not "short direction")

#### CLI Integration

```bash
./target/debug/cs campaign \
    --strategy iron-butterfly \
    --wing-mode delta:0.25 \      # NEW
    --direction short \            # NEW, default
    --period-policy pre-earnings \
    --start 2025-11-03 \
    --end 2025-11-03 \
    --hedge --hedge-strategy time \
    --output-dir ./results
```

**Wing-mode Parser**:
```
Format examples:
  "delta:0.25"      → Delta { wing_delta: 0.25 }
  "moneyness:0.10"  → Moneyness { wing_percent: 0.10 }
  (default)         → Delta { wing_delta: 0.25 } if unspecified
```

#### Strategy-Specific Direction Handling

All strategies support `--direction long/short` with uniform semantics:

| Strategy | Short (Default) | Long |
|----------|-----------------|------|
| **Straddle** | Sell both legs | Buy both legs |
| **CalendarSpread** | Short near + Long far | Long near + Short far (diagonal) |
| **IronButterfly** | Short ATM + Long wings | Long ATM + Short wings |
| **CalendarStraddle** | Short near + Long far | Long near + Short far |

**Implementation Pattern**:
1. Parse `--direction` flag in CLI
2. Pass to factory: `factory.create_iron_butterfly_advanced(..., direction)`
3. If direction == Long, invert all leg positions before building
4. Validation rules adapt based on direction

---

## Implementation Steps

### Step 1: Create Wing Selection Configuration Types

**File**: `cs-domain/src/value_objects/wing_selection.rs` (NEW)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WingSelectionMode {
    Delta { wing_delta: f64 },
    Moneyness { wing_percent: f64 },
}

#[derive(Debug, Clone)]
pub struct IronButterflyConfig {
    pub wing_mode: WingSelectionMode,
    pub symmetric: bool,
}

impl IronButterflyConfig {
    pub fn default_delta() -> Self {
        Self {
            wing_mode: WingSelectionMode::Delta { wing_delta: 0.25 },
            symmetric: true,
        }
    }

    pub fn from_cli_arg(arg: &str) -> Result<Self, String> {
        // Parse "delta:0.25" or "moneyness:0.10"
        // ...
    }
}
```

Update `cs-domain/src/value_objects/mod.rs` to export new types.

### Step 2: Add TradeDirection Enum

**File**: `cs-domain/src/value_objects/trade_direction.rs` (NEW)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeDirection {
    Long,
    Short,
}

impl Default for TradeDirection {
    fn default() -> Self {
        TradeDirection::Short
    }
}

impl From<&str> for TradeDirection {
    fn from(s: &str) -> Self {
        match s {
            "long" => TradeDirection::Long,
            _ => TradeDirection::Short,
        }
    }
}
```

### Step 3: Implement Delta-Based Strike Selector

**File**: `cs-domain/src/strike_selection/delta_symmetric.rs` (NEW)

```rust
use cs_analytics::DeltaVolSurface;

pub struct DeltaSymmetricIronButterflyStrategy;

impl StrikeSelector for DeltaSymmetricIronButterflyStrategy {
    fn select_iron_butterfly(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        wing_delta: f64,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        // 1. Build DeltaVolSurface
        let delta_surface = DeltaVolSurface::from_iv_surface(surface, 0.02)?;

        // 2. Select expiration by DTE
        let expirations = select_expirations_by_dte(surface, min_dte, max_dte)?;
        let expiration = *expirations.iter().min().ok_or(SelectionError::NoExpirations)?;

        // 3. Find ATM strike
        let strikes = surface.strikes();
        let center = find_closest_strike(&strikes, spot.value)?;

        // 4. Get vol slice for expiration
        let vol_slice = delta_surface.get_vol_slice(expiration)?;

        // 5. Convert deltas to strikes
        let upper_strike = vol_slice.delta_to_strike(wing_delta, true)?;  // call
        let lower_strike = vol_slice.delta_to_strike(-wing_delta, false)?;  // put

        // 6. Validate symmetric
        validate_symmetric(&center, &upper_strike, &lower_strike)?;

        // 7. Build and return IronButterfly
        build_iron_butterfly(...)
    }
}

// Variant for moneyness-based
pub struct MoneynessSymmetricIronButterflyStrategy;

impl StrikeSelector for MoneynessSymmetricIronButterflyStrategy {
    fn select_iron_butterfly(
        &self,
        spot: &SpotPrice,
        surface: &IVSurface,
        wing_percent: f64,
        min_dte: i32,
        max_dte: i32,
    ) -> Result<IronButterfly, SelectionError> {
        // Similar pattern but using moneyness instead of delta
        let center = find_closest_strike(&surface.strikes(), spot.value)?;
        let upper_target = spot.value * Decimal::from_f64_retain(1.0 + wing_percent)?;
        let lower_target = spot.value * Decimal::from_f64_retain(1.0 - wing_percent)?;

        // Snap to available strikes...
    }
}
```

### Step 4: Extend IronButterfly Entity

**File**: `cs-domain/src/entities.rs`

Add `direction` field:
```rust
pub struct IronButterfly {
    pub short_call: OptionLeg,
    pub short_put: OptionLeg,
    pub long_call: OptionLeg,
    pub long_put: OptionLeg,
    pub direction: TradeDirection,  // NEW
}

impl IronButterfly {
    pub fn new(
        short_call: OptionLeg,
        short_put: OptionLeg,
        long_call: OptionLeg,
        long_put: OptionLeg,
        direction: TradeDirection,
    ) -> Result<Self, ValidationError> {
        // Existing validation...
        // Plus: validate matches direction
        // For Long: short_call.strike > long_call.strike (inverted logic)
    }
}
```

### Step 5: Extend TradeFactory Trait

**File**: `cs-domain/src/ports/trade_factory.rs`

Add new method:
```rust
async fn create_iron_butterfly_advanced(
    &self,
    symbol: &str,
    as_of: DateTime<Utc>,
    min_expiration: NaiveDate,
    config: &IronButterflyConfig,
    direction: TradeDirection,
) -> Result<IronButterfly, TradeFactoryError>;
```

### Step 6: Implement in DefaultTradeFactory

**File**: `cs-backtest/src/trade_factory_impl.rs`

```rust
async fn create_iron_butterfly_advanced(
    &self,
    symbol: &str,
    as_of: DateTime<Utc>,
    min_expiration: NaiveDate,
    config: &IronButterflyConfig,
    direction: TradeDirection,
) -> Result<IronButterfly, TradeFactoryError> {
    // 1. Query option chain
    let chain = self.options_repo
        .get_option_bars_at_time(symbol, as_of)
        .await?;

    // 2. Build IV surface
    let surface = build_iv_surface(&chain, &*self.equity_repo, symbol).await?;

    // 3. Extract spot price
    let spot_price = SpotPrice::new(
        Decimal::try_from(surface.spot_price())?,
        as_of,
    );

    // 4. Create appropriate selector based on wing mode
    let butterfly = match &config.wing_mode {
        WingSelectionMode::Delta { wing_delta } => {
            let selector = DeltaSymmetricIronButterflyStrategy;
            selector.select_iron_butterfly(&spot_price, &surface, *wing_delta, ...)?
        }
        WingSelectionMode::Moneyness { wing_percent } => {
            let selector = MoneynessSymmetricIronButterflyStrategy;
            selector.select_iron_butterfly(&spot_price, &surface, *wing_percent, ...)?
        }
    };

    // 5. If direction is Long, invert leg positions
    let final_butterfly = if direction == TradeDirection::Long {
        invert_iron_butterfly_for_long(butterfly)?
    } else {
        butterfly
    };

    Ok(final_butterfly)
}

fn invert_iron_butterfly_for_long(ib: IronButterfly) -> Result<IronButterfly, TradeFactoryError> {
    // Swap short_call ↔ long_call
    // Swap short_put ↔ long_put
    Ok(IronButterfly::new(
        ib.long_call,    // now short
        ib.long_put,     // now short
        ib.short_call,   // now long
        ib.short_put,    // now long
        TradeDirection::Long,
    )?)
}
```

### Step 7: Update IronButterfly::create() in rollable_impls.rs

**File**: `cs-domain/src/entities/rollable_impls.rs`

```rust
#[async_trait]
impl RollableTrade for IronButterfly {
    type Result = IronButterflyResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &IronButterflyConfig,
        direction: TradeDirection,
    ) -> Result<Self, TradeConstructionError> {
        factory
            .create_iron_butterfly_advanced(symbol, dt, min_expiration, config, direction)
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    // ... rest of impl
}
```

### Step 8: Add CLI Parsing

**File**: `cs-cli/src/main.rs`

Add to CampaignArgs struct:
```rust
#[derive(Parser)]
pub struct CampaignArgs {
    // ... existing fields ...

    /// Direction: "short" (default) or "long"
    #[arg(long, default_value = "short")]
    pub direction: String,

    /// Wing positioning: "delta:0.25" or "moneyness:0.10"
    #[arg(long, default_value = "delta:0.25")]
    pub wing_mode: String,
}
```

Add parser function:
```rust
fn parse_wing_mode(arg: &str) -> Result<IronButterflyConfig> {
    IronButterflyConfig::from_cli_arg(arg)
        .map_err(|e| anyhow::anyhow!("Invalid wing-mode: {}", e))
}

fn parse_direction(arg: &str) -> Result<TradeDirection> {
    match arg {
        "long" => Ok(TradeDirection::Long),
        "short" => Ok(TradeDirection::Short),
        other => Err(anyhow::anyhow!("Invalid direction '{}': use 'long' or 'short'", other)),
    }
}
```

### Step 9: Thread Through Campaign Executor

**Files**: Campaign execution path (likely `session_executor.rs` and `trade_executor.rs`)

Pass `config` and `direction` from CLI → CampaignArgs → SessionExecutor → IronButterfly::create()

---

## CLI Usage Examples

### Delta-based, 25-delta symmetric, short (default):
```bash
./target/debug/cs campaign \
    --strategy iron-butterfly \
    --wing-mode delta:0.25 \
    --direction short \
    --period-policy pre-earnings \
    --start 2025-11-03 --end 2025-11-03 \
    --output-dir ./results_delta_25
```

### Moneyness-based, 10% OTM symmetric, long:
```bash
./target/debug/cs campaign \
    --strategy iron-butterfly \
    --wing-mode moneyness:0.10 \
    --direction long \
    --period-policy pre-earnings \
    --start 2025-11-03 --end 2025-11-03 \
    --output-dir ./results_moneyness_long
```

### 15-delta (tighter wings), short:
```bash
./target/debug/cs campaign \
    --strategy iron-butterfly \
    --wing-mode delta:0.15 \
    --period-policy pre-earnings \
    --start 2025-11-03 --end 2025-11-03 \
    --output-dir ./results_delta_15
```

---

## Testing & Validation

### Unit Tests
- [ ] Delta-to-strike conversion accuracy
- [ ] Moneyness calculation and snapping
- [ ] Symmetric constraint validation
- [ ] Direction inversion (short ↔ long)
- [ ] IronButterflyConfig parsing from CLI

### Integration Tests
```bash
# Test delta-based selection
./target/debug/cs campaign \
    --strategy iron-butterfly \
    --wing-mode delta:0.25 \
    --period-policy pre-earnings \
    --start 2025-11-03 --end 2025-11-03 \
    --output-dir ./test_delta_wings

# Verify output JSON:
# - Strikes follow delta pattern
# - Symmetric constraint: |upper - center| ≈ |center - lower|
```

### Validation Checklist
- [ ] 25-delta calls found correctly via DeltaVolSurface
- [ ] 25-delta puts found correctly
- [ ] Moneyness: upper_strike/spot ≈ 1.10, lower_strike/spot ≈ 0.90
- [ ] Symmetric: (upper - center) ≈ (center - lower) within tolerance
- [ ] Long direction: legs inverted (long ATM + short wings)
- [ ] Short direction: standard (short ATM + long wings)
- [ ] CLI parsing works for all formats

---

## Key Files to Modify

### New Files (6)
- `cs-domain/src/value_objects/wing_selection.rs` - WingSelectionMode, IronButterflyConfig
- `cs-domain/src/value_objects/trade_direction.rs` - TradeDirection enum
- `cs-domain/src/strike_selection/delta_symmetric.rs` - Strike selection strategies
- `cs-domain/src/strike_selection/moneyness_symmetric.rs` - Or combined in above

### Modified Files (8)
- `cs-domain/src/value_objects/mod.rs` - export new types
- `cs-domain/src/entities.rs` - add direction to IronButterfly
- `cs-domain/src/entities/rollable_impls.rs` - update IronButterfly::create() signature
- `cs-domain/src/ports/trade_factory.rs` - add trait method
- `cs-backtest/src/trade_factory_impl.rs` - implement factory method
- `cs-cli/src/main.rs` - CLI parsing
- Campaign executor layer - thread config/direction through
- Test files - validation tests

---

## Risk Assessment

**Medium Risk Areas**:
- DeltaVolSurface edge case handling
- Delta extrapolation beyond available strikes
- Long variant Greeks inversion logic
- Trait signature changes (RollableTrade::create)

**Mitigations**:
- Comprehensive error handling for failed conversions
- Fallback to moneyness if delta fails
- Unit tests for Greeks in both directions
- Validation that snapped strikes satisfy symmetric constraint (tolerance: ±0.50)

---

## Future Enhancements

These can be implemented later as separate tasks:

- `--wing-upper-delta 0.25 --wing-lower-delta 0.20` - asymmetric deltas
- `--wing-constraint cost-optimized|vega-neutral|gamma-balanced` - optimization metrics
- `--iv-surface-model sticky-strike|sticky-delta` - IV dynamics assumptions
- Multi-expiration butterflies (short near-term, long far-term with different expirations)
- Automatic wing width optimization based on earnings volatility

---

## Implementation Order

1. **Phase 1**: Create config types and TradeDirection enum (2 files)
2. **Phase 2**: Implement strike selectors (delta + moneyness) (2 files)
3. **Phase 3**: Extend factory trait and implement (2 files)
4. **Phase 4**: Update IronButterfly entity and rollable_impls (2 files)
5. **Phase 5**: CLI parsing and integration (3 files)
6. **Phase 6**: Testing and validation

**Estimated scope**: 9 new files + 8 modified files, ~1500 LOC

---

**Plan created**: 2026-01-08
**Status**: Ready for implementation approval
