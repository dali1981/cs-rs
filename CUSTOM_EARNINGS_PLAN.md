# Custom Earnings File Support - Implementation Plan

## Goal
Allow users to provide custom earnings dates via a file (parquet or JSON) for backtesting, separate from the `--earnings-dir` option which uses the earnings-rs adapter.

## Current State
- `--earnings-dir` → Uses `EarningsReaderAdapter` which wraps `earnings-rs::EarningsReader`
- `earnings-rs` expects a specific directory structure and TradingView data format
- No way to provide a simple list of custom earnings dates

## Proposed Solution

### New CLI Option
Add `--earnings-file` option that accepts a path to a single file:
```bash
cs backtest --earnings-file ./my_earnings.parquet ...
cs backtest --earnings-file ./my_earnings.json ...
```

### Option Behavior
| Option | Source | Reader |
|--------|--------|--------|
| `--earnings-dir` | earnings-rs data directory | `EarningsReaderAdapter` |
| `--earnings-file` | User-provided parquet/JSON | `CustomFileEarningsReader` |

**Mutual Exclusivity**: `--earnings-dir` and `--earnings-file` are mutually exclusive. If both provided, error out.

### File Formats

#### Parquet Schema (preferred)
```
symbol: String
earnings_date: Date
earnings_time: String ("BMO" | "AMC")
company_name: String (optional)
market_cap: UInt64 (optional)
```

#### JSON Schema (alternative)
```json
[
  {"symbol": "ANGO", "date": "2025-01-08", "time": "BMO"},
  {"symbol": "ANGO", "date": "2025-04-02", "time": "BMO"}
]
```

## Implementation Steps

### Step 1: Create Reader (DONE)
File: `cs-domain/src/infrastructure/custom_parquet_earnings.rs`
- `CustomParquetEarningsReader` - reads from a directory containing parquet files
- Already created, but needs to be modified to read a single file

### Step 2: Rename and Refactor Reader
Rename to `CustomFileEarningsReader` and support:
- Single parquet file
- Single JSON file
- Auto-detect format by extension

### Step 3: Export from Module
File: `cs-domain/src/infrastructure/mod.rs`
- Add `pub mod custom_file_earnings;`
- Add `pub use custom_file_earnings::CustomFileEarningsReader;`

### Step 4: Update CLI Args
File: `cs-cli/src/main.rs`

Add to `Commands::Backtest`:
```rust
/// Custom earnings file (parquet or JSON) - mutually exclusive with --earnings-dir
#[arg(long, conflicts_with = "earnings_dir")]
earnings_file: Option<PathBuf>,
```

### Step 5: Update run_backtest Function
File: `cs-cli/src/main.rs`

```rust
// Determine earnings source
let earnings_repo: Box<dyn EarningsRepository> = match (&earnings_dir, &earnings_file) {
    (Some(dir), None) => Box::new(EarningsReaderAdapter::new(dir.clone())),
    (None, Some(file)) => Box::new(CustomFileEarningsReader::from_file(file.clone())?),
    (None, None) => return Err(anyhow!("Either --earnings-dir or --earnings-file is required")),
    (Some(_), Some(_)) => unreachable!(), // conflicts_with handles this
};
```

### Step 6: Update BacktestUseCase
File: `cs-backtest/src/backtest_use_case.rs`

Change constructor to accept `Box<dyn EarningsRepository>` instead of concrete type:
```rust
pub fn new(
    earnings_repo: Box<dyn EarningsRepository>,  // Changed from concrete type
    options_repo: FinqOptionsRepository,
    equity_repo: FinqEquityRepository,
    config: BacktestConfig,
) -> Self
```

### Step 7: Python Helper Script (DONE)
File: `scripts/create_custom_earnings.py`
- Already created
- Generates parquet files with correct schema

## Usage Examples

### Create Custom Earnings File
```bash
python scripts/create_custom_earnings.py ANGO \
  "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO" \
  --output ./ango_earnings.parquet
```

### Run Backtest with Custom Earnings
```bash
./target/debug/cs backtest \
  --earnings-file ./ango_earnings.parquet \
  --symbols ANGO \
  --start 2025-01-01 \
  --end 2025-12-31 \
  --spread straddle
```

### Generate ATM IV and Plot
```bash
# Generate IV time series
./target/debug/cs atm-iv --symbols ANGO --start 2025-01-01 --end 2025-12-31 --constant-maturity --output ./iv_output

# Plot with earnings
python scripts/plotting/plot_atm_iv_with_earnings.py \
  ./iv_output/atm_iv_ANGO.parquet \
  --earnings "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO" \
  --output ango_iv_2025.png
```

## Files to Modify

1. `cs-domain/src/infrastructure/custom_parquet_earnings.rs` → Rename & refactor to single-file reader
2. `cs-domain/src/infrastructure/mod.rs` → Export new reader
3. `cs-cli/src/main.rs` → Add `--earnings-file` option, update run_backtest
4. `cs-backtest/src/backtest_use_case.rs` → Accept Box<dyn EarningsRepository>

## Testing

1. Unit test: `CustomFileEarningsReader` loads parquet correctly
2. Unit test: `CustomFileEarningsReader` loads JSON correctly
3. Integration test: Backtest with custom earnings file finds opportunities
4. Manual test: ANGO 2025 backtest with provided earnings dates
