# Trade Replay Analyzer Guide

The trade replay analyzer reconstructs market context for individual trades, allowing you to visualize and analyze why trades succeeded or failed in relation to market dynamics.

## Features

- **Stock Price Context**: Chart spot price movement with entry/exit and earnings event markers
- **IV Evolution**: Track implied volatility changes throughout the trade period
- **Volatility Comparison**: Historical vs Entry IV visualization
- **Greeks Analysis**: Display Delta, Gamma, Theta, Vega at trade entry
- **P&L Summary**: Quick view of profits, loss percentages, and IV changes

## Installation

Dependencies are already included in `pyproject.toml`. Just sync:

```bash
uv sync
```

## Usage

### Analyze a Single Trade from JSON Results

```bash
uv run python3 replay_trade.py --result backtest_results.json
```

If the JSON file contains multiple trades (array), it will analyze the first one. Specify an index:

```bash
uv run python3 replay_trade.py --result backtest_results.json --index 3
```

### Analyze from Parquet Results

```bash
uv run python3 replay_trade.py --result results.parquet --index 0
```

### Specify Output Location

```bash
uv run python3 replay_trade.py --result results.json --output my_analysis.png
```

### Use Custom finq Data Directory

```bash
uv run python3 replay_trade.py --result results.json --data-dir ~/custom_finq_data
```

## Output

The tool generates a PNG file with 5 analysis panels:

### Panel 1: Stock Price (Top, Full Width)
- Blue line: Spot price movement
- Green vertical line + marker: Entry point
- Red vertical line + marker: Exit point
- Orange vertical line: Earnings event (if applicable)

### Panel 2 (Bottom Left): IV Evolution
- Plots the ATM implied volatility at entry and exit dates
- Shows IV term structure or crush/spike if available
- Marked with entry/exit lines

### Panel 3 (Bottom Right): Volatility Comparison
- 30-day historical volatility (HV) over the trade period
- Entry IV level as a horizontal reference line
- Helps understand realized vs expected volatility

### Panel 4 (Bottom Left): Greeks Analysis
- Table of entry-time Greeks (Delta, Gamma, Theta, Vega)
- Useful for understanding position sensitivity

### Panel 5 (Bottom Right): P&L Summary
- Total P&L in dollars
- P&L as percentage of capital
- Entry and exit IV levels
- IV change (positive = expansion, negative = crush)
- Trade status (Success/Failed)

## Data Requirements

The tool needs:

1. **Trade Result File** (JSON or Parquet)
   - Must contain: `symbol`, `entry_time`, `exit_time`, `earnings_date`
   - Optional: `pnl`, `pnl_pct`, `iv_entry`, `iv_exit`, `spot_at_entry`, `spot_at_exit`, Greeks

2. **finq Market Data**
   - Default location: `~/finq_data`
   - Must contain EOD quotes and option chain data for the symbol
   - Data should cover the period from 30 days before entry to 1 day after exit

## Example Workflow

### 1. Run a backtest and save results

```bash
cargo run --bin cs -- backtest \
  --conf config.yaml \
  --start 2025-01-01 \
  --end 2025-01-31 \
  --spread iron-butterfly \
  --output backtest_results.json
```

### 2. Identify an interesting trade

```bash
# View the results
cat backtest_results.json | python3 -m json.tool | head -100
```

### 3. Replay the trade with full market context

```bash
uv run python3 replay_trade.py --result backtest_results.json --index 0
```

### 4. Analyze the generated visualization

The PNG file will contain all the market context, helping you understand:
- Whether IV crush or expansion dominated the P&L
- How spot price movement affected Greeks
- Whether earnings event timing aligned with trade entry/exit
- How historical vs realized volatility compared

## Troubleshooting

### "No spot data available"
- Ensure `~/finq_data` contains EOD quote data for the symbol
- Check that finq is properly configured with your data directory
- Verify the symbol is correct in the trade result

### "No option chain data available"
- Option chains may not be available for all symbols
- The analysis will still work with spot data alone
- IV metrics will be missing from the visualization

### "finq library not found"
- Install dependencies: `uv sync`
- Ensure finq path in `pyproject.toml` points to your polygon directory

## Advanced: Batch Analysis

To analyze multiple trades at once, you can create a simple loop:

```bash
for i in {0..9}; do
  echo "Analyzing trade $i..."
  uv run python3 replay_trade.py --result results.parquet --index $i
done
```

Or extend the script to accept multiple indices and generate a summary report.

## Interpreting Results

### Successful Trades Often Show:
- Entry near IV peaks (if short volatility)
- Entry before earnings events (if long volatility before crush)
- IV behavior matching the trade thesis
- Spot movement that benefits position Greeks

### Failed Trades Often Show:
- Entry near IV troughs (for short volatility trades)
- Unexpected large spot moves
- IV behavior opposite to thesis
- High realized volatility despite low entry IV

## Next Steps

Use insights from individual trade replays to:
1. Identify market conditions that favor your strategy
2. Refine entry/exit timing rules
3. Understand which volatility metrics are predictive
4. Build filters to avoid unfavorable setups
