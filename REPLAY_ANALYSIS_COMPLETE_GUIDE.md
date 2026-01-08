# Trade Replay & Analysis - Complete Guide

A comprehensive system for understanding **why** individual trades succeeded or failed by replaying them through real market context data.

## Table of Contents

1. [Overview](#overview)
2. [System Components](#system-components)
3. [Installation & Setup](#installation--setup)
4. [Single Trade Analysis](#single-trade-analysis)
5. [Batch Analysis](#batch-analysis)
6. [Interpreting Results](#interpreting-results)
7. [Workflows](#workflows)
8. [Troubleshooting](#troubleshooting)

---

## Overview

### The Problem

Backtest results show aggregate statistics, but individual trade success/failure is a mystery:
- Why did this trade profit while a similar one lost?
- Was it IV crush or spot movement?
- Did earnings timing matter?
- Were the Greeks aligned with realized moves?

### The Solution

**Trade Replay** reconstructs the market context for each trade:
- Stock price movement with entry/exit markers
- Implied volatility evolution (from option chains)
- Historical vs realized volatility comparison
- Greeks behavior (delta, gamma, theta, vega)
- Complete P&L attribution

All visualized in an easy-to-interpret 5-panel chart.

### Key Insight

Market context reveals the **true drivers** of trade P&L:
- ✅ Did IV behave as expected? → Vega P&L
- ✅ Did spot move within Greeks limits? → Delta/Gamma P&L
- ✅ Did time decay help? → Theta P&L
- ✅ Was entry/exit timing critical? → Market condition matching

---

## System Components

### 1. `replay_trade.py` - Single Trade Analyzer

Analyzes one trade in detail with market context.

**Inputs:**
- Trade result (JSON or Parquet)
- Market data from finq (automatic)

**Outputs:**
- PNG file with 5-panel analysis
- Console summary

**Use when:**
- Deep diving into a specific trade
- Understanding anomalies
- Verifying strategy thesis

### 2. `batch_replay_trades.py` - Batch Processor

Analyzes multiple trades with filtering.

**Inputs:**
- Trade results file (JSON or Parquet)
- Filter criteria (symbols, P&L, success/failure)

**Outputs:**
- Multiple PNG files (one per trade)
- HTML index linking all analyses
- Progress summary

**Use when:**
- Analyzing all winners/losers
- Identifying patterns
- Creating reports

### 3. `TRADE_REPLAY_GUIDE.md` - Quick Reference

Quick-start guide with common commands.

---

## Installation & Setup

### Prerequisites

```bash
# Ensure dependencies are installed
uv sync
```

This installs:
- `matplotlib` - visualization
- `numpy` - calculations
- `polars` - data handling
- `finq` - market data access

### Market Data

Trade replay requires market data from **finq**:

```bash
# Default location: ~/finq_data
# To use custom location, pass --data-dir flag
```

Ensure finq is configured with:
- EOD stock quotes (for spot price history)
- Option chain snapshots (for IV data)

Data should cover: **30 days before entry through 1 day after exit**

---

## Single Trade Analysis

### Basic Usage

Analyze the first trade from a backtest:

```bash
uv run python3 replay_trade.py --result backtest_results.json
```

### Select Specific Trade

If results contain multiple trades (array in JSON or rows in Parquet):

```bash
# Analyze 3rd trade (0-indexed)
uv run python3 replay_trade.py --result results.parquet --index 2

# From JSON array
uv run python3 replay_trade.py --result results.json --index 5
```

### Custom Output Location

```bash
uv run python3 replay_trade.py \
  --result results.json \
  --output my_analysis_aapl.png
```

### Custom Data Directory

```bash
uv run python3 replay_trade.py \
  --result results.json \
  --data-dir ~/custom_finq_data
```

### Full Example

```bash
uv run python3 replay_trade.py \
  --result backtest_results.parquet \
  --index 0 \
  --output analysis_aapl_entry.png \
  --data-dir /mnt/finq_backup
```

---

## Batch Analysis

### Process All Trades

```bash
uv run python3 batch_replay_trades.py --result results.parquet
```

Generates:
- `trade_replays/trade_SYMBOL_DATETIME_idxN.png` for each trade
- `trade_replays/index.html` linking all trades

### Filter by Success/Failure

```bash
# Only winners
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --success-only

# Only losers
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --failed-only
```

### Filter by Symbols

```bash
# Only AAPL, MSFT, GOOGL trades
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --symbols AAPL MSFT GOOGL
```

### Filter by P&L

```bash
# Trades between $100 and $500 profit
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --min-pnl 100 \
  --max-pnl 500

# Losers worse than -$50
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --max-pnl -50

# Winners better than +100%
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --min-pnl-pct 100
```

### Limit Number of Trades

```bash
# Process only first 10 trades (useful for sampling large backtests)
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --max-trades 10
```

### Parallel Processing

```bash
# Use 8 worker threads (default is 4)
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --workers 8
```

### Combined Filters

```bash
# Top 5 winners in TECH symbols (AAPL, MSFT, TSLA)
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --symbols AAPL MSFT TSLA \
  --success-only \
  --max-trades 5
```

### Output Customization

```bash
# Custom output directory + skip HTML index
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --output-dir ./analysis_q4_2025 \
  --no-index
```

---

## Interpreting Results

### Panel 1: Stock Price (Top)

**What it shows:**
- Blue line: Spot price movement over the trade period
- Green marker: Trade entry point
- Red marker: Trade exit point
- Orange vertical line: Earnings event (if applicable)

**What to look for:**

✅ **Good setup:**
- Entry near local peak (for short vol strategies)
- Entry before earnings with upside/downside protection
- Spot stays near entry strikes → Greeks neutral

⚠️ **Problematic setup:**
- Entry near local trough (for short vol)
- Entry after earnings event
- Spot moves large distance → Greeks hurt position

**Example interpretation:**
```
Stock trades $100 → $105 (+5%)
Short strangle at $95 put / $105 call:
  - PUT: Out of money → good
  - CALL: In the money → very bad
  Result: Delta loss dominates, position fails
```

---

### Panel 2: IV Evolution (Bottom Left)

**What it shows:**
- ATM (at-the-money) implied volatility at entry and exit dates
- Vertical lines mark entry/exit times

**What to look for:**

✅ **Successful short-vol trades:**
- Entry IV near peak (expanded from baseline)
- IV crushes toward exit
- IV change explains most of P&L

✅ **Successful long-vol trades:**
- Entry IV near trough (compressed)
- IV expands toward exit
- IV expansion drives profit

⚠️ **Mismatched trades:**
- Entry IV near term low → selling vol at worst time
- Exit IV unchanged or higher → strategy thesis violated

**Example interpretation:**
```
Entry IV: 35% (near 6-month high)
Exit IV:  28% (after earnings)
IV crush: -700bp → Large vega profit for short vol
Trade result: +$800 profit (mostly vega, minimal theta)
```

---

### Panel 3: Volatility Comparison (Bottom Right)

**What it shows:**
- Blue line: 30-day Historical Volatility (HV)
- Orange horizontal line: Entry IV level
- Green/Red vertical lines: Entry/Exit times

**What to look for:**

✅ **IV > HV at entry:**
- Selling vol above realized levels (good for short vol)
- Strategy captures vol premium
- Actual moves likely smaller than entry IV implied

⚠️ **IV < HV at entry:**
- Selling vol below realized (bad for short vol)
- Actual moves likely bigger than entry IV implied
- Greeks blow up more than expected

✅ **HV constant during hold:**
- Actual moves match expectations
- Greeks behave as modeled

⚠️ **HV spikes during hold:**
- Spot moves larger than expected
- Delta/Gamma losses mount even if overall IV ok

**Example interpretation:**
```
Entry IV: 40%
Entry HV: 25%
Realized HV during trade: 45%

Interpretation: We sold vol 15pp above baseline
But actual realized vol was 5pp higher than we sold
Result: Greeks hurt more than vega helped → Loss
```

---

### Panel 4: Greeks Analysis (Bottom Left)

**What it shows:**
- Delta: Directional sensitivity ($1 stock move impact)
- Gamma: Delta acceleration (sensitivity to big moves)
- Theta: Time decay benefit (+vega benefit per day)
- Vega: Vol sensitivity ($1 IV change impact)

**What to look for:**

✅ **Balanced Greeks:**
- Delta near 0 (direction neutral)
- Positive Theta (time decay helps)
- Vega aligned with view (negative for short vol)

⚠️ **Imbalanced:**
- Large Delta → directional bet (not neutral)
- Negative Theta → paying for time
- Vega opposite to strategy → working against you

**Example interpretation:**
```
Strangle Greeks:
  Delta:  +0.05 → Slightly bullish
  Gamma:  +0.001 → Benefits from moves
  Theta:  +0.08/day → Makes $0.08/day from decay
  Vega:   +0.30 → Loses if IV expands

For short-vol trade: Negative vega is wrong!
Should have negative vega → shows risk management issue
```

---

### Panel 5: P&L Summary (Bottom Right)

**What it shows:**
- Total P&L in dollars
- P&L as percentage
- IV at entry and exit
- IV change (positive = expansion, negative = crush)
- Trade status (Success/Failed)

**What to look for:**

✅ **Successful trade interpretation:**
- P&L aligns with Greeks thesis
  - IV crush? → Vega profit dominates
  - Limited spot move? → Theta profit
  - Spot explosion? → Gamma profit
- Failure reason is None
- P&L % shows risk-adjusted return

⚠️ **Failed trade interpretation:**
- Failure reason explains what went wrong
- P&L inconsistent with Greeks
  - Should profit but lost? → Greeks changed unexpectedly
  - Should lose more but limited? → Lucky spot placement
- P&L % shows capital inefficiency

**Example interpretation:**
```
P&L: +$450 (12%)
IV Entry: 35%
IV Exit:  28%
IV Change: -700bp → Vega gained ~$500
Status: Success

Analysis: Trade profits almost entirely from IV crush
Spot movement and theta were secondary
Strategy thesis confirmed: volatility mean reversion worked
```

---

## Workflows

### Workflow 1: Understanding a Single Trade

**Goal:** Deep dive into why a specific trade succeeded or failed

```bash
# 1. List backtest results to find interesting trade
head -20 backtest_results.json

# 2. Replay the trade with market context
uv run python3 replay_trade.py \
  --result backtest_results.json \
  --index 3 \
  --output trade_analysis.png

# 3. Open trade_analysis.png and analyze:
#    - Does spot price action match expectations?
#    - Did IV behave as strategy required?
#    - Are Greeks exposed to realized conditions?
#    - Does P&L explanation make sense?

# 4. Compare to similar trades that failed
uv run python3 replay_trade.py \
  --result backtest_results.json \
  --index 7

# 5. Identify the difference
#    - Same strategy, different outcome
#    - What changed? Timing? Market conditions? Greeks?
```

### Workflow 2: Analyzing Winners

**Goal:** Find patterns in profitable trades

```bash
# 1. Generate replays for all winners
uv run python3 batch_replay_trades.py \
  --result backtest_results.parquet \
  --success-only \
  --max-trades 20

# 2. Open trade_replays/index.html in browser
#    Review all winners side-by-side

# 3. Look for common patterns:
#    - Do winners enter on IV spikes?
#    - Do they all have IV crush?
#    - Is spot movement consistently limited?
#    - Do certain symbols dominate?

# 4. Screenshot or note the successful patterns
#    Example: "Winners average +8pp IV crush, enter at IV peaks"

# 5. Refine strategy rules based on findings
#    Add filters: "Only enter when IV > 75th percentile"
```

### Workflow 3: Analyzing Losers

**Goal:** Understand failure modes

```bash
# 1. Generate replays for worst losers
uv run python3 batch_replay_trades.py \
  --result backtest_results.parquet \
  --failed-only \
  --max-pnl -100 \
  --max-trades 10

# 2. Review failure patterns:
#    - Did Greeks blow up? → Need position sizing
#    - Did IV move opposite to thesis? → Need better entry timing
#    - Did spot explode? → Need wider wings
#    - Did entry timing matter? → Need earnings awareness

# 3. Categorize failures
#    - "Greeks failure" (14 trades)
#    - "IV expansion failure" (6 trades)
#    - "Spot explosion failure" (8 trades)

# 4. Address top failure mode
#    - If Greeks: Reduce position size or use wider strikes
#    - If IV: Add pre-earnings IV filters
#    - If spot: Increase strike width or use iron condors
```

### Workflow 4: Symbol-Specific Analysis

**Goal:** Find which symbols are most profitable

```bash
# 1. Analyze by symbol
uv run python3 batch_replay_trades.py \
  --result backtest_results.parquet \
  --symbols AAPL \
  --success-only

uv run python3 batch_replay_trades.py \
  --result backtest_results.parquet \
  --symbols AAPL \
  --failed-only

# 2. Compare results
#    - AAPL: 70% winners (strong!)
#    - MSFT: 50% winners (neutral)
#    - TSLA: 30% winners (weak)

# 3. Understand why
#    - Look at AAPL winner characteristics
#    - Look at TSLA loser characteristics
#    - IV patterns? Spot volatility? Greeks exposure?

# 4. Weight portfolio toward high-probability symbols
```

### Workflow 5: Strategy Refinement

**Goal:** Iteratively improve strategy rules

**Before:**
```yaml
strategy: strangle
entry_iv: any
min_dte: 30
wing_delta: 0.25
```

**Step 1: Analyze winners**
```bash
uv run python3 batch_replay_trades.py \
  --result v1_results.parquet \
  --success-only
# Finding: 90% entered when IV > 60th percentile
```

**Step 2: Refine rules**
```yaml
strategy: strangle
entry_iv: "> 60th_percentile"  # NEW FILTER
min_dte: 30
wing_delta: 0.25
```

**Step 3: Backtest new version**
```bash
cargo run --bin cs -- backtest --conf v2_config.yaml --output v2_results.json
```

**Step 4: Analyze new results**
```bash
uv run python3 batch_replay_trades.py \
  --result v2_results.parquet \
  --success-only
# Check if win rate improved
```

**Step 5: Repeat until satisfied**

---

## Troubleshooting

### "No spot data available"

**Cause:** Market data not found in finq directory

**Solution:**
```bash
# Check data exists
ls ~/finq_data/AAPL/

# Use explicit data directory
uv run python3 replay_trade.py \
  --result results.json \
  --data-dir /path/to/finq_data

# Ensure finq is configured correctly
# Check finq documentation for data requirements
```

### "No option chain data available"

**Cause:** Option chain snapshots not available for trade dates

**Acceptable:** Analysis will work without option chains
- IV panel will be empty
- All other panels will show data
- This is not a fatal error

**Fix if needed:**
```bash
# Ensure finq has option chains for entry/exit dates
# May need to run finq data download for those dates
```

### "Error loading market data"

**Cause:** finq library error or data corruption

**Debugging:**
```bash
# Test finq directly
uv run python3 -c "
import finq
data = finq.fetch_eod_quotes('AAPL', '2025-01-01', '2025-01-31')
print(data)
"

# If fails, check finq logs and configuration
```

### "Trade not found" / "Index out of range"

**Cause:** Specified trade index doesn't exist

**Solution:**
```bash
# Check number of trades in file
uv run python3 -c "
import polars as pl
df = pl.read_parquet('results.parquet')
print(f'Total trades: {len(df)}')
"

# Use valid index (0 to count-1)
uv run python3 replay_trade.py \
  --result results.parquet \
  --index 5  # If count=10, use 0-9
```

### "Memory error" with batch processing

**Cause:** Too many parallel workers

**Solution:**
```bash
# Reduce workers
uv run python3 batch_replay_trades.py \
  --result large_results.parquet \
  --workers 2  # Instead of 4
```

### "matplotlib cannot save PNG"

**Cause:** Permission issue or invalid output path

**Solution:**
```bash
# Ensure output directory exists and is writable
mkdir -p ./output
chmod 755 ./output

# Specify full path
uv run python3 replay_trade.py \
  --result results.json \
  --output /full/path/to/analysis.png
```

---

## Advanced Usage

### Generate Reproducible Reports

```bash
# Batch analyze with date-stamped output
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
uv run python3 batch_replay_trades.py \
  --result backtest_results.parquet \
  --output-dir "analysis_$TIMESTAMP" \
  --success-only
```

### Automated Trade Analysis Pipeline

```bash
#!/bin/bash
# analyze_backtest.sh

RESULTS_FILE="$1"
OUTPUT_DIR="analysis_$(date +%Y%m%d)"

mkdir -p "$OUTPUT_DIR"

# Analyze all trades
uv run python3 batch_replay_trades.py \
  --result "$RESULTS_FILE" \
  --output-dir "$OUTPUT_DIR" \
  --workers 8

# Copy results
cp "$OUTPUT_DIR/index.html" "$OUTPUT_DIR/index_all.html"

# Analyze winners
uv run python3 batch_replay_trades.py \
  --result "$RESULTS_FILE" \
  --output-dir "$OUTPUT_DIR/winners" \
  --success-only \
  --workers 8

# Analyze losers
uv run python3 batch_replay_trades.py \
  --result "$RESULTS_FILE" \
  --output-dir "$OUTPUT_DIR/losers" \
  --failed-only \
  --workers 8

echo "Analysis complete: $OUTPUT_DIR"
```

Usage:
```bash
chmod +x analyze_backtest.sh
./analyze_backtest.sh backtest_results.parquet
```

### Compare Two Strategy Versions

```bash
# Strategy v1 vs v2 comparison
echo "=== Strategy v1 ==="
uv run python3 batch_replay_trades.py --result v1_results.parquet --max-trades 5
echo ""
echo "=== Strategy v2 ==="
uv run python3 batch_replay_trades.py --result v2_results.parquet --max-trades 5
```

---

## Tips & Best Practices

### 1. **Start with Extreme Cases**

Analyze the:
- Biggest winner
- Biggest loser
- Most recent trade
- Most common scenario

```bash
# Extract best/worst
uv run python3 -c "
import polars as pl
df = pl.read_parquet('results.parquet')
print('BEST:', df.select('symbol', 'pnl', 'entry_time').max())
print('WORST:', df.select('symbol', 'pnl', 'entry_time').min())
"
```

### 2. **Focus on Unrealized Assumptions**

When analyzing, ask:
- Was IV actually where I expected it?
- Did spot move as I modeled?
- Did Greeks behave as predicted?

The answer reveals modeling gaps.

### 3. **Use Batch for Pattern Detection**

Single trades show outliers. Batches show patterns.

```bash
# Batch winners
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --success-only \
  --max-trades 50

# Review 50 winners → see common threads
# Example: "80% entered within 5 days of earnings"
```

### 4. **Share Analysis with Others**

The PNG output is self-contained and explanation is visual:

```bash
# Generate analysis for team review
uv run python3 replay_trade.py \
  --result results.json \
  --index 12 \
  --output team_review_trade_12.png

# Email/share PNG → colleagues instantly understand the trade context
```

### 5. **Iterate Strategy Rules**

Use analysis to refine:

```
Observation: 85% of winners have IV > 65th percentile at entry
Action: Add "IV > 65th percentile" filter to strategy config
Result: Next backtest shows improved win rate from 55% → 62%
```

---

## Summary

| Tool | Use Case | Input | Output |
|------|----------|-------|--------|
| `replay_trade.py` | Deep dive, single trade | 1 trade (JSON/Parquet) | 1 PNG (5-panel) |
| `batch_replay_trades.py` | Pattern detection, reports | Multiple trades + filters | Many PNGs + HTML index |

**Typical Workflow:**
1. Run backtest → results.parquet
2. Batch analyze winners → Identify patterns
3. Deep dive on specific trades → Understand mechanisms
4. Refine strategy rules → Update config
5. Repeat

**Key Value:**
- Transforms aggregate statistics into actionable insights
- Reveals gaps between model and reality
- Identifies profitable market conditions
- Enables evidence-based strategy refinement

---

## Questions?

For issues or questions:
1. Check **Troubleshooting** section above
2. Review **TRADE_REPLAY_GUIDE.md** for quick reference
3. Check that finq is properly configured and has required data
4. Verify trade results file format (JSON or Parquet)

Happy analyzing! 📊
