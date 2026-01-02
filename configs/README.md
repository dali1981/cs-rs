# Strategy Configuration Files

This directory contains pre-configured strategy files for common backtest scenarios.

## Quick Start

### Iron Butterfly with Delta Selection

```bash
./target/release/cs backtest \
  --conf iron_butterfly_delta.toml \
  --start 2025-11-01 \
  --end 2025-11-30
```

### Calendar Spread with Delta Selection

```bash
./target/release/cs backtest \
  --conf calendar_delta.toml \
  --option-type call \
  --start 2025-11-01 \
  --end 2025-11-30
```

## Available Configs

### `iron_butterfly_delta.toml`
- **Spread**: Iron Butterfly (sell ATM straddle, buy wings)
- **Selection**: Delta targeting (0.50 = ATM)
- **Wing Width**: $10.00
- **Min IV Ratio**: 1.5 (back-month IV / front-month IV)
- **Use Case**: Profit from time decay in high IV environment

### `calendar_delta.toml`
- **Spread**: Calendar (sell near-term, buy far-term)
- **Selection**: Delta targeting (0.50 = ATM-ish)
- **Min IV Ratio**: 1.5 (back-month IV / front-month IV)
- **Use Case**: Profit from IV term structure and time decay
- **Note**: Requires `--option-type call` or `--option-type put`

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
spread_type = "calendar" | "iron_butterfly"
selection_type = "atm" | "delta" | "delta_scan"
target_delta = 0.50              # For delta/delta_scan strategies
wing_width = 10.0                # For iron_butterfly only

[selection]
min_short_dte = 3
max_short_dte = 45
min_long_dte = 14
max_long_dte = 90
min_iv_ratio = 1.5               # Optional IV ratio filter

[pricing]
model = "sticky_strike" | "sticky_moneyness" | "sticky_delta"
vol_model = "linear" | "svi"

[timing]
entry_hour = 9
entry_minute = 35
exit_hour = 15
exit_minute = 55
```
