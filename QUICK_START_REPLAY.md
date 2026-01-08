# Trade Replay - Quick Start (2-Minute Guide)

## Install

```bash
uv sync  # Install matplotlib and numpy
```

## Essential Commands

### Analyze 1 Trade
```bash
uv run python3 replay_trade.py --result results.json
```

### Analyze Specific Trade
```bash
uv run python3 replay_trade.py --result results.parquet --index 5
```

### Batch Analyze Winners
```bash
uv run python3 batch_replay_trades.py --result results.parquet --success-only
```

### Batch Analyze Losers
```bash
uv run python3 batch_replay_trades.py --result results.parquet --failed-only
```

### Filter by Symbols
```bash
uv run python3 batch_replay_trades.py --result results.parquet --symbols AAPL MSFT TSLA
```

### Filter by P&L
```bash
# Winners over $100
uv run python3 batch_replay_trades.py --result results.parquet --min-pnl 100

# Losers worse than -$50
uv run python3 batch_replay_trades.py --result results.parquet --max-pnl -50
```

## Output

**Single Trade:**
- PNG file with 5 panels showing stock price, IV, volatility, Greeks, P&L

**Batch:**
- PNG files in `trade_replays/` directory
- `trade_replays/index.html` - clickable index of all trades

## The 5 Panels Explained

1. **Stock Price** - Entry (green) and exit (red) on price chart
2. **IV Evolution** - Volatility changes during trade
3. **Volatility Comparison** - Historical vol vs IV level
4. **Greeks** - Delta, Gamma, Theta, Vega at entry
5. **P&L Summary** - Dollar profit, percentage, IV metrics

## Quick Interpretation

**Winners usually have:**
- IV crush (panel 2 goes down) → Vega profit
- Limited spot move (panel 1 stays calm) → Greeks profit
- Time decay (panel 4 positive Theta) → Theta profit

**Losers usually have:**
- IV expansion opposite to trade thesis
- Spot explosion (panel 1 spikes) → Greeks blow up
- Earnings event (orange line) → unexpected volatility

## Common Workflows

### Find Why This Trade Won
```bash
uv run python3 replay_trade.py --result results.json --index 0
# Open PNG → look at IV and spot panels
# Did IV crush help? Did spot stay calm?
```

### Find Patterns in Winners
```bash
uv run python3 batch_replay_trades.py --result results.parquet --success-only --max-trades 20
# Open trade_replays/index.html
# Review 20 winners → do they all have IV crushes? Same symbols?
```

### Find Why These Trades Lost
```bash
uv run python3 batch_replay_trades.py --result results.parquet --failed-only --max-pnl -100
# Open PNG files
# What went wrong? Spot explosion? IV expansion? Greeks hit?
```

## Requirements

- **finq market data** in `~/finq_data` (or specify `--data-dir`)
- **Trade results file** (JSON or Parquet from backtest)
- Data covering 30 days before entry through 1 day after exit

## Troubleshooting

| Problem | Solution |
|---------|----------|
| "No spot data" | Ensure finq has EOD data for symbol |
| "No IV data" | Optional - analysis works without it |
| "Index out of range" | Use valid index: 0 to (total trades - 1) |
| "Permission denied" | `mkdir -p output && chmod 755 output` |

## Full Documentation

See `REPLAY_ANALYSIS_COMPLETE_GUIDE.md` for:
- Detailed panel interpretation
- Advanced filtering
- Strategy refinement workflows
- All troubleshooting steps

## Real Example

```bash
# 1. Run backtest
cargo run --bin cs -- backtest \
  --conf config.yaml \
  --start 2025-01-01 \
  --end 2025-01-31 \
  --output results.parquet

# 2. Find best trade
uv run python3 replay_trade.py --result results.parquet --index 0
# Opens: trade_replay_AAPL_20250103_1430.png

# 3. Find patterns in all winners
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --success-only
# Opens: trade_replays/index.html

# 4. Compare winners vs losers
# Review both index files side-by-side
# Notice: winners have IV > 60th percentile
# Update strategy config with this filter
```

---

**Next:** Read `REPLAY_ANALYSIS_COMPLETE_GUIDE.md` for comprehensive details
