# Trade Direction (Long/Short) Implementation Status

**Report Date**: 2026-01-09
**Status**: ✅ **PARTIALLY IMPLEMENTED AND WORKING**

## Summary

The ability to select **Long** or **Short** positions for option strategies (Straddle, IronButterfly, etc.) is **implemented and working in the Campaign system**, but **NOT wired up in the standard Backtest system**.

---

## Implementation Details

### ✅ Domain Layer - **COMPLETE**

The domain layer fully supports trade direction:

**1. TradeDirection Enum** (`cs-domain/src/value_objects.rs:639-654`)
```rust
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
        match s.to_lowercase().as_str() {
            "long" => TradeDirection::Long,
            _ => TradeDirection::Short,
        }
    }
}
```

**2. TradingCampaign** (`cs-domain/src/campaign/campaign.rs:47`)
```rust
pub struct TradingCampaign {
    // ... other fields
    pub trade_direction: TradeDirection,
}
```

**3. TradingSession** (`cs-domain/src/campaign/session.rs:37`)
```rust
pub struct TradingSession {
    // ... other fields
    pub trade_direction: TradeDirection,
}
```

**4. TradeFactory Port** (`cs-domain/src/ports/trade_factory.rs:120-127`)
```rust
async fn create_iron_butterfly_advanced(
    &self,
    symbol: &str,
    as_of: DateTime<Utc>,
    min_expiration: NaiveDate,
    config: &IronButterflyConfig,
    direction: TradeDirection,  // ← Direction parameter
) -> Result<IronButterfly, TradeFactoryError>;
```

**5. Strike Selection** (`cs-domain/src/strike_selection/atm.rs:544`)
```rust
fn select_iron_butterfly_with_config(
    &self,
    spot: &SpotPrice,
    surface: &IVSurface,
    config: &IronButterflyConfig,
    direction: TradeDirection,  // ← Direction parameter
    min_dte: i32,
    max_dte: i32,
) -> Result<IronButterfly, SelectionError>
```

The selection logic properly inverts leg positions based on direction:
- **Short (default)**: Short ATM straddle + Long OTM wings
- **Long**: Long ATM straddle + Short OTM wings

---

### ✅ Campaign System - **FULLY WIRED AND WORKING**

**1. CampaignConfig** (`cs-backtest/src/campaign_config.rs:27`)
```rust
pub struct CampaignConfig {
    // ... other fields
    pub trade_direction: TradeDirection,
}
```

**2. CLI Support** (`cs campaign --help`)
```bash
--direction <DIRECTION>
    Trade direction (short or long) - applies to all strategies [default: short]
```

**3. Session Executor** (`cs-backtest/src/session_executor.rs:643-648`)
```rust
// Properly passes direction from session to factory
self.trade_factory.create_iron_butterfly_advanced(
    &session.symbol,
    session.entry_datetime,
    session.exit_date(),
    config,
    session.trade_direction,  // ← Direction from TradingSession
).await
```

**Example Usage (WORKS):**
```bash
target/release/cs campaign \
  --symbols AAPL MSFT \
  --strategy iron-butterfly \
  --direction long \
  --start 2025-01-01 \
  --end 2025-12-31
```

---

### ❌ Standard Backtest System - **NOT WIRED**

**Missing Components:**

**1. BacktestConfig** (`cs-backtest/src/config/mod.rs:18-92`)
```rust
pub struct BacktestConfig {
    // ... 92 lines of config fields
    // ❌ NO trade_direction field
}
```

**2. CLI Args** (`cs-cli/src/args/`)
- No `--direction` argument in `BacktestArgs`
- No `TradeDirection` field in `StrategyArgs`

**3. Config Files**
- TOML configs don't support `direction = "long"` or `direction = "short"`

**Current Behavior:**
```bash
# This command does NOT support --direction
target/release/cs backtest \
  --start 2025-01-01 --end 2025-12-31 \
  --spread straddle \
  # ❌ No --direction flag available
```

All backtests default to `TradeDirection::Short` implicitly.

---

## What Works vs What Doesn't

### ✅ Works (Campaign System)
- CLI `--direction` flag
- Long and short iron butterflies
- Direction propagates through TradingSession → TradeFactory → StrikeSelection
- Leg positions properly inverted for long strategies

### ❌ Doesn't Work (Backtest System)
- No CLI argument for direction
- No config file support
- No BacktestConfig field
- Cannot test long straddles or long butterflies via standard backtest
- No way to override default Short direction

---

## Architecture Gap

The issue is architectural:

1. **Campaign System** uses:
   - `TradingCampaign` → `TradingSession` (includes direction)
   - Sessions executed by `SessionExecutor`

2. **Backtest System** uses:
   - `BacktestConfig` → `BacktestUseCase` → `TradeExecutor`
   - No direction field in execution path

The **domain layer** supports direction, but the **backtest infrastructure** doesn't wire it up.

---

## Recommendations

### Option 1: Add Direction to BacktestConfig (Recommended)

**Changes needed:**
1. Add `trade_direction` field to `BacktestConfig`
2. Add `--direction` CLI argument to `BacktestArgs`
3. Add `direction` field to TOML config support
4. Update `BacktestUseCase` to pass direction to trade factory
5. Update all trade factory calls in `trade_executor.rs`

**Impact**: Medium (5-6 files to modify)

**Benefits**:
- Consistent with Campaign system
- Enables long strategy backtests
- Backward compatible (defaults to Short)

### Option 2: Migrate to Campaign System

Deprecate `BacktestUseCase` in favor of `CampaignUseCase`:
- Campaign system already has direction support
- More flexible (period policies, roll policies)
- Better architecture (sessions as atomic units)

**Impact**: Large (would be a breaking change)

**Benefits**:
- Eliminates duplicate code paths
- Single source of truth for execution
- All features automatically work

### Option 3: Do Nothing

Keep direction only in Campaign system:
- Use `cs campaign` for long strategies
- Use `cs backtest` for short strategies only

**Impact**: None

**Drawbacks**:
- Confusing for users
- Feature disparity between commands
- Technical debt accumulates

---

## Testing Checklist

### ✅ Tested and Working
- [x] Campaign with `--direction short` (default)
- [x] Campaign with `--direction long` (inverts positions)
- [x] Iron butterfly long/short variants
- [x] Direction propagates through TradingSession
- [x] Strike selection respects direction

### ❌ Not Tested (Not Implemented)
- [ ] Backtest with direction flag (doesn't exist)
- [ ] TOML config with direction field (not supported)
- [ ] Long straddles via backtest command
- [ ] Long calendar spreads via backtest command

---

## Code References

| Component | File | Line | Status |
|-----------|------|------|--------|
| TradeDirection enum | `cs-domain/src/value_objects.rs` | 639 | ✅ Implemented |
| TradingCampaign | `cs-domain/src/campaign/campaign.rs` | 47 | ✅ Has field |
| TradingSession | `cs-domain/src/campaign/session.rs` | 37 | ✅ Has field |
| CampaignConfig | `cs-backtest/src/campaign_config.rs` | 27 | ✅ Has field |
| BacktestConfig | `cs-backtest/src/config/mod.rs` | 18-92 | ❌ Missing field |
| Campaign CLI | `cs-cli/src/args/campaign.rs` | ? | ✅ Has --direction |
| Backtest CLI | `cs-cli/src/args/backtest.rs` | ? | ❌ No --direction |
| TradeFactory | `cs-domain/src/ports/trade_factory.rs` | 120 | ✅ Has parameter |
| Strike Selection | `cs-domain/src/strike_selection/atm.rs` | 544 | ✅ Uses direction |
| Session Executor | `cs-backtest/src/session_executor.rs` | 643 | ✅ Passes direction |

---

## Conclusion

**The TradeDirection feature is implemented in the domain and working in the Campaign system, but not wired up in the standard Backtest system.**

To use long positions today: **Use `cs campaign --direction long`**

To add long positions to standard backtests: **Implement Option 1 above**

---

*Generated by analysis of cs-rs-new-approach codebase*
