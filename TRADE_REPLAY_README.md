# Trade Replay & Analysis System

A complete system for analyzing individual trades through market context visualization. Understand **why** trades succeeded or failed by replaying them through real market data.

## 📚 Documentation Structure

### Getting Started
- **[QUICK_START_REPLAY.md](QUICK_START_REPLAY.md)** - 2-minute quick reference
  - Essential commands
  - Basic workflows
  - Troubleshooting table
  - **Start here if you want to begin immediately**

### Complete Reference
- **[REPLAY_ANALYSIS_COMPLETE_GUIDE.md](REPLAY_ANALYSIS_COMPLETE_GUIDE.md)** - Comprehensive 880+ line guide
  - System overview and problem/solution
  - Installation and setup
  - Single trade analysis (detailed)
  - Batch analysis (all filtering options)
  - 5 common workflows
  - Advanced usage and automation
  - Full troubleshooting
  - **Read this for complete understanding**

### Visual Interpretation
- **[PANEL_INTERPRETATION_GUIDE.md](PANEL_INTERPRETATION_GUIDE.md)** - Visual patterns and examples
  - Detailed interpretation of each 5 panels
  - Pattern recognition (✅ good vs ⚠️ bad patterns)
  - Real-world examples with diagrams
  - What to track across trades
  - **Use this when analyzing actual trade replays**

---

## 🛠️ Tools

### `replay_trade.py` - Single Trade Analyzer

Analyze one trade in detail with market context reconstruction.

**Typical Usage:**
```bash
# Analyze first trade
uv run python3 replay_trade.py --result backtest_results.json

# Analyze specific trade
uv run python3 replay_trade.py --result results.parquet --index 5 --output my_analysis.png
```

**Output:** PNG file with 5-panel visualization

**Use when:** Deep diving into specific trade, understanding anomalies, verifying strategy

### `batch_replay_trades.py` - Batch Processor

Analyze multiple trades with filtering and parallel processing.

**Typical Usage:**
```bash
# All winners
uv run python3 batch_replay_trades.py --result results.parquet --success-only

# Specific symbols
uv run python3 batch_replay_trades.py --result results.parquet --symbols AAPL MSFT TSLA

# P&L range
uv run python3 batch_replay_trades.py --result results.parquet --min-pnl 100 --max-pnl 500

# Top 10 trades
uv run python3 batch_replay_trades.py --result results.parquet --max-trades 10
```

**Output:** Multiple PNG files + HTML index linking all trades

**Use when:** Pattern detection, creating reports, analyzing winners/losers

---

## 📊 The 5-Panel Visualization

Each trade replay generates 5 analysis panels:

| Panel | Shows | Key Insight |
|-------|-------|-------------|
| **1. Stock Price** | Spot movement with entry/exit markers | Did Greeks stay safe? |
| **2. IV Evolution** | Implied volatility changes | Did IV crush or expand? |
| **3. HV vs IV** | Historical vol vs entry IV | Who won the volatility bet? |
| **4. Greeks** | Delta, Gamma, Theta, Vega | Was position thesis correct? |
| **5. P&L Summary** | Trade result and metrics | What drove the P&L? |

**Learn to interpret:** See [PANEL_INTERPRETATION_GUIDE.md](PANEL_INTERPRETATION_GUIDE.md)

---

## 🚀 Quick Start

### 1. Install
```bash
uv sync
```

### 2. Analyze a Trade
```bash
uv run python3 replay_trade.py --result backtest_results.json
```

### 3. View Output
Open the generated PNG file to see:
- Stock price movement with entry/exit
- IV evolution during trade
- Volatility comparison
- Greeks at entry
- P&L summary

### 4. Understand the Pattern
Refer to [PANEL_INTERPRETATION_GUIDE.md](PANEL_INTERPRETATION_GUIDE.md) to interpret each panel

### 5. Analyze More Trades
```bash
# All winners
uv run python3 batch_replay_trades.py --result results.parquet --success-only

# Open trade_replays/index.html
```

---

## 🎯 Common Workflows

### Understand Why a Trade Won
```bash
uv run python3 replay_trade.py --result results.json --index 0
# Open PNG → Look at IV and spot panels
# Did IV crush help? Did spot stay calm?
```

### Find Patterns in Winners
```bash
uv run python3 batch_replay_trades.py --result results.parquet --success-only
# Open index.html → Review all winners
# Do they all have IV crushes? Same symbols?
```

### Find Failure Modes
```bash
uv run python3 batch_replay_trades.py --result results.parquet --failed-only
# What went wrong? Spot explosion? IV expansion?
```

### Compare Winners vs Losers
```bash
# Winners
uv run python3 batch_replay_trades.py --result results.parquet --success-only --output-dir winners

# Losers
uv run python3 batch_replay_trades.py --result results.parquet --failed-only --output-dir losers

# Review both index.html files side-by-side
```

### Refine Strategy Rules
```bash
# 1. Analyze winners → find common pattern
#    Example: "90% entered when IV > 60th percentile"

# 2. Update strategy config with filter

# 3. Backtest new version
cargo run --bin cs -- backtest --conf v2_config.yaml --output v2_results.json

# 4. Analyze new results → check improvement

# 5. Repeat until satisfied
```

---

## 💡 Key Insights

### Successful Trades Usually Show

```
Panel 1: Flat or slow move         → Greeks safe
Panel 2: IV crush (-5pp or more)   → Vega profit
Panel 3: IV > HV at entry          → Won vol bet
Panel 4: Pos Theta, Neg Vega       → Thesis correct
Panel 5: Green P&L                 → Profit achieved
```

### Failed Trades Usually Show

```
Panel 1: Big spot move             → Greeks blow up
Panel 2: IV expansion              → Vega loss
Panel 3: HV spike, IV < HV at entry → Lost vol bet
Panel 4: High Gamma, Low Theta     → Risky setup
Panel 5: Red P&L                   → Loss realized
```

---

## 📋 Requirements

### Market Data
- **finq data directory** (default: `~/finq_data`)
- **EOD stock quotes** for symbol
- **Option chain data** (optional, for IV analysis)
- **Coverage:** 30 days before entry → 1 day after exit

### Backtest Results
- **JSON file** with single trade or array of trades
- **Parquet file** with multiple trades (one row per trade)

**Required fields in trade result:**
- `symbol` - Stock symbol (e.g., "AAPL")
- `entry_time` - ISO datetime (e.g., "2025-01-15T09:30:00Z")
- `exit_time` - ISO datetime
- `earnings_date` - ISO date (optional, e.g., "2025-01-15")

**Optional but recommended:**
- `pnl` - Trade profit/loss
- `pnl_pct` - P&L percentage
- `iv_entry`, `iv_exit` - Implied volatility levels
- `spot_at_entry`, `spot_at_exit` - Spot prices
- `net_delta`, `net_gamma`, `net_theta`, `net_vega` - Greeks

---

## 🔧 Installation & Dependencies

### Requirements
- Python 3.11+
- `uv` package manager
- `finq` library (for market data)

### Install
```bash
# Sync dependencies (matplotlib, numpy, polars, finq)
uv sync
```

### Verify
```bash
# Test single trade analysis
uv run python3 replay_trade.py --help

# Test batch analysis
uv run python3 batch_replay_trades.py --help
```

---

## 🚨 Troubleshooting

| Issue | Solution |
|-------|----------|
| "No spot data available" | Ensure finq has EOD data for symbol |
| "No option chain data" | Optional - tool works without it |
| "Index out of range" | Use valid index (0 to total-1) |
| "Permission denied" | `mkdir -p output && chmod 755 output` |
| finq errors | Check finq configuration and data directory |

**Full troubleshooting guide:** See [REPLAY_ANALYSIS_COMPLETE_GUIDE.md](REPLAY_ANALYSIS_COMPLETE_GUIDE.md#troubleshooting)

---

## 📖 Documentation Map

```
Start Here
    ↓
[QUICK_START_REPLAY.md]
    ↓
Run: uv run python3 replay_trade.py --result results.json
    ↓
View: Generated PNG
    ↓
[PANEL_INTERPRETATION_GUIDE.md]
    ↓
Interpret each panel
    ↓
Want more details?
    ↓
[REPLAY_ANALYSIS_COMPLETE_GUIDE.md]
    ↓
Full workflows, advanced usage, automation
```

---

## 📞 Getting Help

1. **Quick answers?**
   - Check [QUICK_START_REPLAY.md](QUICK_START_REPLAY.md)

2. **Can't interpret panels?**
   - See [PANEL_INTERPRETATION_GUIDE.md](PANEL_INTERPRETATION_GUIDE.md)

3. **Running into issues?**
   - Check Troubleshooting in [REPLAY_ANALYSIS_COMPLETE_GUIDE.md](REPLAY_ANALYSIS_COMPLETE_GUIDE.md#troubleshooting)

4. **Want complete reference?**
   - Read [REPLAY_ANALYSIS_COMPLETE_GUIDE.md](REPLAY_ANALYSIS_COMPLETE_GUIDE.md)

---

## 🎓 Learning Path

**Day 1: Get Started**
1. Read [QUICK_START_REPLAY.md](QUICK_START_REPLAY.md) (5 min)
2. Run first analysis (5 min)
3. Review generated PNG (5 min)

**Day 2: Understand Patterns**
1. Read [PANEL_INTERPRETATION_GUIDE.md](PANEL_INTERPRETATION_GUIDE.md) (20 min)
2. Analyze 10 trades with batch processor (10 min)
3. Identify winner/loser patterns (15 min)

**Day 3: Apply to Strategy**
1. Review [REPLAY_ANALYSIS_COMPLETE_GUIDE.md](REPLAY_ANALYSIS_COMPLETE_GUIDE.md) workflows (15 min)
2. Implement filters based on patterns
3. Backtest new version
4. Compare results

---

## 🎯 What You'll Learn

After using this system, you'll understand:

✅ Which market conditions favor your strategy
✅ How IV behavior affects P&L
✅ Whether Greeks behave as expected
✅ When entry/exit timing matters
✅ Which symbols are most profitable
✅ How to refine strategy rules with evidence

---

## 📝 Example Real-World Usage

```bash
# 1. Run backtest
cargo run --bin cs -- backtest \
  --conf config.yaml \
  --start 2025-01-01 \
  --end 2025-01-31 \
  --output results.parquet

# 2. Analyze winners
uv run python3 batch_replay_trades.py \
  --result results.parquet \
  --success-only

# 3. Review index.html - notice pattern:
#    "90% of winners have IV > 60th percentile at entry"

# 4. Update strategy config
#    Add: entry_iv_filter: "> 60th_percentile"

# 5. Backtest new version
cargo run --bin cs -- backtest \
  --conf config_v2.yaml \
  --start 2025-02-01 \
  --end 2025-02-28 \
  --output results_v2.parquet

# 6. Check improvement
# Win rate: 55% → 62% ✓

# 7. Continue refining...
```

---

## 💬 Next Steps

1. **Read:** [QUICK_START_REPLAY.md](QUICK_START_REPLAY.md)
2. **Run:** `uv run python3 replay_trade.py --result results.json`
3. **Learn:** [PANEL_INTERPRETATION_GUIDE.md](PANEL_INTERPRETATION_GUIDE.md)
4. **Analyze:** `uv run python3 batch_replay_trades.py --result results.parquet --success-only`
5. **Refine:** Apply patterns to strategy configuration
6. **Validate:** Backtest improvements and repeat

---

**Happy analyzing! 📊**

For comprehensive reference, see [REPLAY_ANALYSIS_COMPLETE_GUIDE.md](REPLAY_ANALYSIS_COMPLETE_GUIDE.md)
