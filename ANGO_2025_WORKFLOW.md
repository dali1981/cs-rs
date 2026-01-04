# ANGO 2025 Custom Earnings Workflow

Complete workflow for analyzing and backtesting custom earnings dates with ATM IV visualization.

## Input Data
Symbol: **ANGO**
Earnings dates for 2025:
- 08-01-2025 BMO (Q1)
- 02-04-2025 BMO (Q2)
- 17-07-2025 BMO (Q3)
- 02-10-2025 BMO (Q4)

---

## Step 1: Create Custom Earnings File

```bash
python scripts/create_custom_earnings.py ANGO \
  "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO" \
  --output ./custom_earnings/earnings_2025.parquet
```

**Output:**
```
Created earnings file: custom_earnings/earnings_2025.parquet

Earnings events for ANGO:
--------------------------------------------------
  1. 2025-01-08 BMO
  2. 2025-04-02 BMO
  3. 2025-07-17 BMO
  4. 2025-10-02 BMO
```

---

## Step 2: Generate ATM IV Time Series

```bash
export FINQ_DATA_DIR=~/polygon/data

./target/debug/cs atm-iv \
  --symbols ANGO \
  --start 2025-01-01 \
  --end 2025-12-31 \
  --constant-maturity \
  --output ./ango_iv_output
```

**Output:**
```
Processing ANGO...
  365 trading days processed, 243 successful observations
  Saved 243 observations to "./ango_iv_output/atm_iv_ANGO.parquet"
```

---

## Step 3: Plot ATM IV with Earnings Dates

```bash
python scripts/plotting/plot_atm_iv_with_earnings.py \
  ./ango_iv_output/atm_iv_ANGO.parquet \
  --earnings "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO" \
  --output ango_atm_iv_2025.png
```

**Output:**
```
Saved plot to: ango_atm_iv_2025.png

================================================================================
ANGO ATM IV ANALYSIS WITH EARNINGS
================================================================================

Data Range: 2025-01-02 to 2025-12-31
Observations: 243

Earnings Events:
  1. 2025-01-08 BMO: 7d IV=67.6%, 30d IV=70.9%, Spread=-3.3pp
  2. 2025-04-02 BMO: 7d IV=58.8%, 30d IV=57.5%, Spread=+1.3pp
  3. 2025-07-17 BMO: 7d IV=70.2%, 30d IV=70.1%, Spread=+0.1pp
  4. 2025-10-02 BMO: 7d IV=52.0%, 30d IV=49.0%, Spread=+3.0pp

Average IV Levels:
  7d: 68.6%
  14d: 67.7%
  30d: 66.1%
================================================================================
```

**Plot features:**
- 7d, 14d, and 30d constant-maturity IV curves
- Vertical lines marking each earnings date
- Earnings detection spread panel (7d - 30d IV spread)
- Color-coded earnings events by quarter

---

## Step 4: Run Backtest with Custom Earnings

```bash
./target/debug/cs backtest \
  --earnings-file ./custom_earnings/earnings_2025.parquet \
  --symbols ANGO \
  --start 2025-01-01 \
  --end 2025-12-31 \
  --spread straddle \
  --output ./ango_straddle_custom.json
```

**Output:**
```
Configuration:
  Data dir:      "/Users/mohamedali/polygon/data"
  Date range:    2025-01-01 to 2025-12-31
  Spread:        Straddle (Long Volatility)
  Entry:         5 trading days before earnings
  Exit:          1 trading day(s) before earnings
  Symbols:       ["ANGO"]

Using custom earnings file: "./custom_earnings/earnings_2025.parquet"

Results:
+---------------------+------------------+
| Metric              | Value            |
+---------------------+------------------+
| Sessions Processed  | 261              |
+---------------------+------------------+
| Total Opportunities | 4                |
+---------------------+------------------+
| Trades Entered      | 4                |
+---------------------+------------------+
| Win Rate            | 50.00%           |
+---------------------+------------------+
| Total P&L           | $0.59            |
+---------------------+------------------+
| Avg P&L per Trade   | $0.14            |
+---------------------+------------------+
| Mean Return         | 7.22%            |
+---------------------+------------------+
| Sharpe Ratio        | 2.62             |
+---------------------+------------------+
| Avg Winner          | $0.70 (36.78%)   |
+---------------------+------------------+
| Avg Loser           | $-0.40 (-22.33%) |
+---------------------+------------------+

Sample Trades:
  1. ANGO Straddle @ 10 | P&L: $1.35 (71.05%)
  2. ANGO Straddle @ 10 | P&L: $-0.40 (-22.22%)
  3. ANGO Straddle @ 7.5 | P&L: $-0.40 (-22.44%)
  4. ANGO Straddle @ 10 | P&L: $0.05 (2.50%)
```

---

## Key Features Implemented

### 1. Custom Earnings File Support
- **New CLI option:** `--earnings-file` (mutually exclusive with `--earnings-dir`)
- **Supported formats:** Parquet and JSON
- **Auto-detection:** File format detected by extension

### 2. Python Helper Scripts
- **`scripts/create_custom_earnings.py`** - Generate earnings parquet files
- **`scripts/plotting/plot_atm_iv_with_earnings.py`** - Plot IV with earnings markers

### 3. Architecture Changes
- **`CustomFileEarningsReader`** - New reader for user-provided files
- **`BacktestUseCase`** - Modified to accept `Box<dyn EarningsRepository>`
- Clean separation between `earnings-rs` adapter and custom files

---

## File Formats

### Parquet Schema (Preferred)
```
symbol: String
earnings_date: Date
earnings_time: String ("BMO" | "AMC")
company_name: String (optional)
market_cap: UInt64 (optional)
```

### JSON Schema (Alternative)
```json
[
  {"symbol": "ANGO", "date": "2025-01-08", "time": "BMO"},
  {"symbol": "ANGO", "date": "2025-04-02", "time": "BMO"},
  {"symbol": "ANGO", "date": "2025-07-17", "time": "BMO"},
  {"symbol": "ANGO", "date": "2025-10-02", "time": "BMO"}
]
```

---

## Files Created/Modified

### New Files
- `scripts/create_custom_earnings.py` - Earnings file generator
- `scripts/plotting/plot_atm_iv_with_earnings.py` - IV plotter with earnings
- `cs-domain/src/infrastructure/custom_file_earnings.rs` - Custom file reader
- `custom_earnings/earnings_2025.parquet` - ANGO custom earnings
- `ango_atm_iv_2025.png` - ATM IV visualization
- `ango_straddle_custom.json` - Backtest results

### Modified Files
- `cs-cli/src/main.rs` - Added `--earnings-file` CLI option
- `cs-backtest/src/backtest_use_case.rs` - Accept trait object for earnings repo
- `cs-domain/src/infrastructure/mod.rs` - Export `CustomFileEarningsReader`

---

## Usage Tips

1. **Use `--earnings-file` for custom dates** (like specific stock research)
2. **Use `--earnings-dir` for bulk backtests** (when using earnings-rs data)
3. **Plot before backtesting** to visualize IV behavior around earnings
4. **Check the 7d-30d spread** in plots to identify earnings volatility spikes

---

## Next Steps

- Test with multiple symbols in one file
- Add JSON file support testing
- Explore different spread strategies (calendar, iron butterfly)
- Analyze post-earnings straddle performance
