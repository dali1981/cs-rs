# Issue: Earnings Date Discrepancy Between Data Sources

**Date**: 2026-01-09
**Status**: 🔴 Open
**Priority**: Medium
**Component**: EarningsReaderAdapter, Data Infrastructure

---

## Problem

Earnings dates differ between data sources, causing incorrect dates in backtest results.

**Example**: YPF earnings on Nov 7, 2025 (AMC) shows as:
- **TradingView source**: `2025-11-08` ❌
- **Earnings directory source**: `2025-11-07` ✓

**Impact**:
- Backtest JSON output shows wrong `earnings_date` field
- Trade timing calculations are still correct (use TradableEvent resolved dates)
- Reporting and analysis display incorrect earnings dates

---

## Root Cause

`EarningsReaderAdapter::new()` hardcodes `DataSource::TradingView` as default:

```rust
// cs-domain/src/infrastructure/earnings_reader_adapter.rs:20
pub fn new(data_dir: PathBuf) -> Self {
    Self {
        reader: earnings_rs::EarningsReader::new(data_dir),
        source: earnings_rs::DataSource::TradingView,  // ← Hardcoded
    }
}
```

**Why TradingView has wrong date**:
- For AMC (After Market Close) earnings, TradingView may record the "filing date" or "next trading day" instead of the actual market date
- YPF reported AMC on Nov 7, but TradingView shows Nov 8 (next day)

---

## Evidence

### Python script output:
```
Found YPF in: .../tradingview/snapshots/year=2025/month=11/2025-11-08_*.parquet
│ YPF    ┆ 2025-11-08 ┆ ...  # ← TradingView source

Found YPF in: .../earnings/year=2025/month=11/2025-11-07.parquet
│ YPF    ┆ 2025-11-07 ┆ ...  # ← Earnings directory source (correct)
```

### Backtest output:
```json
{
  "symbol": "YPF",
  "earnings_date": "2025-11-08",  // ← Wrong (from TradingView)
  "earnings_time": "AMC",
  "entry_time": "2025-10-30T13:35:00Z",  // ← Correct (resolved by TradingPeriodSpec)
  ...
}
```

---

## Solutions

### Option 1: Change Default Data Source (Quick Fix)
Change `EarningsReaderAdapter::new()` to use the earnings directory source instead of TradingView.

**Pros**:
- One-line fix
- Uses more reliable date source

**Cons**:
- Need to identify correct `DataSource` enum variant in earnings-rs
- May affect other users relying on TradingView source

```rust
pub fn new(data_dir: PathBuf) -> Self {
    Self {
        reader: earnings_rs::EarningsReader::new(data_dir),
        source: earnings_rs::DataSource::???,  // What's the right enum?
    }
}
```

### Option 2: Make Data Source Configurable (Proper Fix)
Add data source selection to configuration.

**Implementation**:
1. Add `earnings_data_source` field to `BacktestConfig`
2. Pass to `RepositoryFactory::create_earnings_repository()`
3. Use `EarningsReaderAdapter::with_source()` instead of `::new()`

**Pros**:
- User can choose data source
- Flexible for different use cases
- Aligns with clean architecture (config-driven infrastructure)

**Cons**:
- Requires config schema change
- Slightly more implementation work

```rust
// In BacktestConfig
pub earnings_data_source: String,  // "tradingview" | "nasdaq" | "earnings"

// In RepositoryFactory
pub fn create_earnings_repository(
    earnings_dir: Option<PathBuf>,
    earnings_file: Option<PathBuf>,
    data_source: &str,
) -> Box<dyn EarningsRepository> {
    if let Some(file) = earnings_file {
        Box::new(ParquetEarningsRepository::new(file.clone()))
    } else if let Some(dir) = earnings_dir {
        let source = match data_source {
            "tradingview" => earnings_rs::DataSource::TradingView,
            "nasdaq" => earnings_rs::DataSource::Nasdaq,  // or whatever
            _ => earnings_rs::DataSource::TradingView,
        };
        Box::new(EarningsReaderAdapter::with_source(dir.clone(), source))
    } else {
        // Default location
        Box::new(EarningsReaderAdapter::new(default_dir))
    }
}
```

### Option 3: Investigate earnings-rs Data Sources
1. Check what data sources are available in earnings-rs
2. Determine which one has the most accurate dates
3. Document the differences for users

---

## Workaround

For now, users can:
1. Use a custom earnings file with correct dates (--earnings-file flag)
2. Ignore the `earnings_date` field in output JSON (trade timing is still correct)

---

## Related Files

- `cs-domain/src/infrastructure/earnings_reader_adapter.rs:20` - Hardcoded DataSource
- `cs-cli/src/factory/repository_factory.rs:38` - Creates EarningsReaderAdapter
- `cs-backtest/src/execution/straddle_impl.rs:113` - Populates earnings_date in result
- `~/trading_project/earnings-rs/` - External dependency managing data sources

---

## Testing

After fix, verify:
```bash
target/debug/cs backtest \
  --start 2025-10-27 --end 2025-10-31 \
  --spread straddle \
  --symbol YPF \
  --output straddle_ypf.json

cat straddle_ypf.json | jq '.earnings_date'
# Should show: "2025-11-07" (not "2025-11-08")
```

---

## Notes

- The refactoring (trade-centric execution) is **working correctly**
- Entry/exit datetimes are resolved properly by `TradingPeriodSpec`
- This is purely a **reporting issue** with the raw earnings_date field
- Does not affect trade execution or P&L calculations

---

*End of issue*
