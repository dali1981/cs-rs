# Minute-Aligned IV Computation Plan

**Date**: January 2, 2025
**Status**: Research Complete - Ready for Implementation

---

## Problem Statement

Current EOD ATM IV computation has a timing mismatch:

1. **Options "close" price** = last trade of the day (could be 2pm, 3pm, etc.)
2. **Spot price** = fetched at EOD (4pm)
3. **IV calculation** uses misaligned data → **incorrect IV**

**Example:**
```
Option last trade: 2pm at $5.00 when AAPL = $150
EOD spot: 4pm AAPL = $155
Current: IV($5.00 option, $155 spot) → WRONG
Correct: IV($5.00 option, $150 spot from 2pm)
```

---

## Existing Infrastructure

### What Already Works

1. **Equity minute bars** (`FinqEquityRepository`):
   - `get_bars(symbol, date)` → minute bars with timestamps
   - `get_spot_price(symbol, target_time)` → point-in-time spot lookup
   - Uses `Timeframe::MINUTE`

2. **Options minute bars** (finq-flatfiles):
   - `get_bars(underlying, Timeframe::MINUTE, from, to)` → minute option trades
   - Has `timestamp` column in nanoseconds
   - Data sorted by timestamp
   - Path: `flatfiles/options/minute_aggs/{year}/{symbol}_{date}.parquet`

3. **Options daily snapshot** (`FinqOptionsRepository`):
   - `get_chain_bars(underlying, date)` → daily aggregated chain
   - Uses `Timeframe::DAY` (current behavior)

### Key Insight

**Options minute data EXISTS** - we just need to expose it through `OptionsDataRepository`.

---

## Implementation Plan

### Phase 1: Extend OptionsDataRepository (Required)

**File**: `cs-domain/src/repositories.rs`

Add new method to trait:
```rust
pub trait OptionsDataRepository: Send + Sync {
    // Existing
    async fn get_option_bars(&self, underlying: &str, date: NaiveDate) -> Result<DataFrame>;

    // NEW: Get option chain snapshot at specific time
    async fn get_option_bars_at_time(
        &self,
        underlying: &str,
        target_time: DateTime<Utc>,
    ) -> Result<DataFrame, RepositoryError>;
}
```

**File**: `cs-domain/src/infrastructure/finq_options_repo.rs`

Implement the new method:
```rust
async fn get_option_bars_at_time(
    &self,
    underlying: &str,
    target_time: DateTime<Utc>,
) -> Result<DataFrame, RepositoryError> {
    let date = target_time.date_naive();

    // Load minute bars (not daily)
    let df = self.repository
        .get_bars(underlying, Timeframe::MINUTE, date, date)
        .await?;

    // Convert target time to nanos
    let target_nanos = TradingTimestamp::from_datetime_utc(target_time).to_nanos();

    // Filter to trades at or before target time
    // Group by (strike, expiration, option_type), take latest per contract
    let filtered = df
        .lazy()
        .filter(col("timestamp").lt_eq(lit(target_nanos)))
        .sort(["timestamp"], SortMultipleOptions::default().with_order_descending(true))
        .group_by([col("strike"), col("expiration"), col("option_type")])
        .agg([
            col("close").first().alias("close"),
            col("timestamp").first().alias("timestamp"),
            // Keep other columns as needed
        ])
        .collect()?;

    Ok(filtered)
}
```

### Phase 2: Create MinuteAlignedIvUseCase

**New File**: `cs-backtest/src/minute_aligned_iv_use_case.rs`

```rust
pub struct MinuteAlignedIvUseCase<E, O> {
    equity_repo: E,
    options_repo: O,
    atm_computer: AtmIvComputer,
}

impl MinuteAlignedIvUseCase {
    /// Compute IV at a specific point in time with aligned prices
    pub async fn compute_iv_at_time(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
        config: &AtmIvConfig,
    ) -> Result<AtmIvObservation, IvTimeSeriesError> {
        // Get spot at target_time (already works)
        let spot = self.equity_repo.get_spot_price(symbol, target_time).await?;

        // Get option chain at target_time (NEW)
        let chain_df = self.options_repo
            .get_option_bars_at_time(symbol, target_time)
            .await?;

        // Compute IV with aligned data
        let options = self.dataframe_to_options(&chain_df)?;
        let results = self.atm_computer.compute_atm_ivs(
            &options,
            spot.to_f64(),
            target_time,
            &config.maturity_targets,
            config.maturity_tolerance,
            config.atm_strike_method,
        );

        // Build observation
        // ...
    }

    /// Generate EOD IV time series using minute-aligned computation
    pub async fn execute(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        config: &AtmIvConfig,
        projection: IvProjectionMethod,
    ) -> Result<IvTimeSeriesResult, IvTimeSeriesError> {
        // For each trading day:
        // 1. Sample IV at multiple points (or use last observation)
        // 2. Project to single EOD value
        // ...
    }
}
```

### Phase 3: IV Projection Methods

**Enum for projection strategy:**
```rust
pub enum IvProjectionMethod {
    /// Use last available observation (simplest, current behavior equivalent)
    LastObservation,

    /// Time-weighted average over trading hours
    TimeWeightedAverage,

    /// Sample at fixed intervals (e.g., every 30 min) and average
    FixedIntervalAverage { interval_minutes: u32 },

    /// Use observation closest to market close
    NearClose { minutes_before_close: u32 },
}
```

**Recommended default**: `LastObservation` or `NearClose { minutes_before_close: 15 }`

### Phase 4: CLI Integration

**File**: `cs-cli/src/main.rs`

Add flag to `atm-iv` command:
```rust
AtmIv {
    // ... existing args ...

    /// Use minute-aligned IV computation (more accurate)
    #[arg(long)]
    minute_aligned: bool,

    /// IV projection method: last, twap, fixed-30, near-close
    #[arg(long, default_value = "last")]
    projection: String,
}
```

---

## Data Flow Comparison

### Current (EOD-based)
```
EOD Options Chain (daily agg) + EOD Spot Price → IV
     ↑                              ↑
     |                              |
  Last trade (unknown time)    4pm spot
```

### New (Minute-aligned)
```
Options at time T + Spot at time T → IV at T
         ↓
Project to EOD (last obs / TWAP / etc.)
```

---

## Testing Strategy

1. **Unit tests**: Mock repos returning known data at specific times
2. **Integration test with AAPL 2025**:
   - Run both EOD and minute-aligned
   - Compare IV values
   - Expect minute-aligned to have less noise/more consistent patterns
3. **Earnings detection comparison**:
   - Run earnings detection with both methods
   - Compare detection rates (should improve with aligned data)

---

## Keep Existing Code Path

**Important**: The existing EOD code path must remain functional:
- `GenerateIvTimeSeriesUseCase` → EOD-based (unchanged)
- `MinuteAlignedIvUseCase` → new minute-aligned approach
- CLI flag selects which to use

---

## Estimated Effort

| Phase | Effort | Risk |
|-------|--------|------|
| Phase 1: Repository extension | 1-2 hours | Low (follows existing pattern) |
| Phase 2: New use case | 2-3 hours | Low (adapts existing logic) |
| Phase 3: Projection methods | 1 hour | Low |
| Phase 4: CLI integration | 30 min | Low |
| Testing & validation | 2 hours | Medium (data availability) |

**Total**: ~7-8 hours

---

## Open Questions

1. **Data availability**: Do we have minute-level options data for AAPL 2025?
   - Need to verify: `flatfiles/options/minute_aggs/2025/AAPL_*.parquet`

2. **Performance**: Minute data is larger - need lazy evaluation and filtering

3. **Sparse data**: Some contracts may not trade every minute
   - Solution: Forward-fill from last known price or skip sparse contracts

---

## Next Steps

1. Verify minute options data exists for test symbols
2. Implement Phase 1 (repository extension)
3. Implement Phase 2 (use case)
4. Test with AAPL 2025 data
5. Compare results with EOD method
