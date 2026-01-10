# Strategy Configuration Files

This directory contains pre-configured strategy files for common backtest scenarios.

## Quick Start

### Short Straddle with IV Filtering

```bash
./target/release/cs backtest \
  --conf configs/short_straddle_iv_filtered.toml \
  --start 2025-11-01 \
  --end 2025-11-30
```

### Short Iron Butterfly with IV Filtering

```bash
./target/release/cs backtest \
  --conf configs/short_iron_butterfly_iv_filtered.toml \
  --start 2025-11-01 \
  --end 2025-11-30
```

### Iron Butterfly with Delta Selection

```bash
./target/release/cs backtest \
  --conf configs/iron_butterfly_delta.toml \
  --start 2025-11-01 \
  --end 2025-11-30
```

### Calendar Spread with Delta Selection

```bash
./target/release/cs backtest \
  --conf configs/calendar_delta.toml \
  --option-type call \
  --start 2025-11-01 \
  --end 2025-11-30
```

## Available Configs

### Short Premium Strategies (with IV Filtering)

#### `short_straddle_iv_filtered.toml`
- **Spread**: Short Straddle (sell ATM call + put)
- **Selection**: ATM
- **Entry Rules**:
  - IV7 > IV30 + 5pp (elevated short-term volatility)
  - Max entry IV < 150% (avoid mispricing)
  - Market cap > $1B (liquidity)
- **Timing**: Enter 20 days before earnings, exit 5 days before
- **Use Case**: Sell volatility when term structure is steep

#### `short_iron_butterfly_iv_filtered.toml`
- **Spread**: Short Iron Butterfly (sell ATM straddle, buy wings)
- **Selection**: Delta targeting (0.50 = ATM)
- **Wing Width**: $10.00
- **Entry Rules**:
  - IV7 > IV30 + 5pp (elevated short-term volatility)
  - Max entry IV < 150% (avoid mispricing)
  - Market cap > $1B (liquidity)
- **Use Case**: Defined-risk volatility selling with IV filtering

### Long Premium Strategies

#### `iron_butterfly_delta.toml`
- **Spread**: Iron Butterfly (sell ATM straddle, buy wings)
- **Selection**: Delta targeting (0.50 = ATM)
- **Wing Width**: $10.00
- **Min IV Ratio**: 1.5 (back-month IV / front-month IV)
- **Use Case**: Profit from time decay in high IV environment

#### `calendar_delta.toml`
- **Spread**: Calendar (sell near-term, buy far-term)
- **Selection**: Delta targeting (0.50 = ATM-ish)
- **Min IV Ratio**: 1.5 (back-month IV / front-month IV)
- **Use Case**: Profit from IV term structure and time decay
- **Note**: Requires `--option-type call` or `--option-type put`

### Straddle Strategies (Legacy)

#### `straddle_1m_before_15d_cap1b_hedged.toml`
- **Spread**: Straddle (long or short)
- **Timing**: Enter 20 days before earnings, exit 5 days before
- **Hedging**: Delta hedging enabled
- **Filters**: Market cap > $1B
- **Use Case**: Hedged volatility exposure

#### `straddle_1m_before_15d_cap1b.toml`
- **Spread**: Straddle (long or short)
- **Timing**: Enter 20 days before earnings, exit 5 days before
- **Filters**: Market cap > $1B
- **Use Case**: Unhedged volatility exposure

#### `straddle_with_costs.toml`
- **Spread**: Straddle with trading costs
- **Use Case**: Test impact of slippage and commissions

## Overriding Config Values

You can override any config value via CLI arguments:

```bash
# Override target delta
./target/release/cs backtest \
  --conf iron_butterfly_delta.toml \
  --target-delta 0.45 \
  --start 2025-11-01 \
  --end 2025-11-30

# Override wing width
./target/release/cs backtest \
  --conf iron_butterfly_delta.toml \
  --wing-width 15 \
  --start 2025-11-01 \
  --end 2025-11-30

# Override DTE ranges
./target/release/cs backtest \
  --conf calendar_delta.toml \
  --option-type call \
  --min-short-dte 7 \
  --max-short-dte 21 \
  --start 2025-11-01 \
  --end 2025-11-30
```

## Configuration Priority

1. **CLI arguments** (highest priority)
2. **Strategy config file** (`--conf`)
3. **System config** (`~/.config/cs/system.toml`)
4. **Code defaults** (lowest priority)

## Creating Custom Configs

Copy an existing config and modify the values:

```bash
cp iron_butterfly_delta.toml my_custom_strategy.toml
# Edit my_custom_strategy.toml
./target/release/cs backtest --conf my_custom_strategy.toml --start ... --end ...
```

## Config File Structure

```toml
[strategy]
spread_type = "calendar" | "iron-butterfly" | "straddle"
selection_type = "atm" | "delta" | "delta_scan"
target_delta = 0.50              # For delta/delta_scan strategies
wing_width = 10.0                # For iron_butterfly only

[selection]
min_short_dte = 3
max_short_dte = 45
min_long_dte = 14
max_long_dte = 90
min_iv_ratio = 1.5               # Optional IV ratio filter (legacy)

[pricing]
model = "sticky_strike" | "sticky_moneyness" | "sticky_delta"
vol_model = "linear" | "svi"

[timing]
entry_hour = 9
entry_minute = 35
exit_hour = 15
exit_minute = 55
# For straddle strategies
straddle_entry_days = 20         # Days before earnings to enter
straddle_exit_days = 5           # Days before earnings to exit

# Entry filters (simple)
min_market_cap = 1_000_000_000   # Minimum market cap in dollars
max_entry_iv = 1.5               # Maximum entry IV (150%)

# Entry rules (advanced - require market data)
[[rules.market]]
type = "iv_slope"
short_dte = 7                    # Short-term DTE for IV
long_dte = 30                    # Long-term DTE for IV
threshold_pp = 0.05              # Minimum slope in percentage points (5pp)

[[rules.market]]
type = "max_entry_iv"
threshold = 1.5                  # Maximum IV (150%)

[[rules.market]]
type = "min_iv_ratio"
short_dte = 7
long_dte = 30
threshold = 1.0                  # Minimum IV_short / IV_long ratio

# Event-level rules
[[rules.event]]
type = "min_market_cap"
threshold = 1_000_000_000        # $1 billion

# Trade-level rules
[[rules.trade]]
type = "entry_price_range"
min = 0.50                       # Minimum entry price
max = 50.0                       # Maximum entry price
```
