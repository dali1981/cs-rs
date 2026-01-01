# Configuration File Examples

This directory contains example configuration files for the calendar spread backtest CLI.

## File Organization

### System Configuration
- `system.toml` - System-wide defaults (place at `~/.config/cs/system.toml`)

### Strategy Configurations
- `aggressive.toml` - Aggressive strategy with higher IV threshold and wider delta scan
- `conservative.toml` - Conservative strategy with tighter filters

## Configuration Layering

Configuration values are loaded in the following priority (highest to lowest):
1. CLI arguments
2. Strategy config file (`--conf`)
3. System config (`~/.config/cs/system.toml`)
4. Code defaults

## Usage Examples

### Basic usage with strategy config
```bash
cs backtest --conf examples/configs/aggressive.toml \
  --start 2024-01-01 --end 2024-06-30
```

### Multiple config files (each merges on top of previous)
```bash
cs backtest --conf examples/configs/system.toml \
  --conf examples/configs/aggressive.toml \
  --start 2024-01-01 --end 2024-06-30
```

### CLI override takes precedence
```bash
cs backtest --conf examples/configs/aggressive.toml \
  --min-iv-ratio 1.5 \
  --start 2024-01-01 --end 2024-06-30
```

### Environment variable overrides for paths
```bash
FINQ_DATA_DIR=/fast/ssd/data \
  cs backtest --conf examples/configs/aggressive.toml \
  --start 2024-01-01 --end 2024-06-30
```

## Configuration Sections

### `[paths]`
- `data_dir` - Market data directory (finq data)
- `earnings_dir` - Earnings data directory

### `[timing]`
- `entry_hour` - Entry hour (0-23)
- `entry_minute` - Entry minute (0-59)
- `exit_hour` - Exit hour (0-23)
- `exit_minute` - Exit minute (0-59)

### `[selection]`
- `min_short_dte` - Minimum days-to-expiry for near leg
- `max_short_dte` - Maximum days-to-expiry for near leg
- `min_long_dte` - Minimum days-to-expiry for far leg
- `max_long_dte` - Maximum days-to-expiry for far leg
- `target_delta` - Optional target delta filter
- `min_iv_ratio` - Optional minimum IV ratio (short/long)

### `[strategy]`
- `type` - Strategy type: `atm`, `delta`, `delta_scan`
- `target_delta` - Target delta (for delta strategies)
- `delta_range` - Delta range for scanning (e.g., `[0.25, 0.75]`)
- `delta_scan_steps` - Number of steps in delta scan

### `[pricing]`
- `model` - Pricing IV interpolation: `sticky_strike`, `sticky_moneyness`, `sticky_delta`
- `vol_model` - Vol smile fitting: `linear`, `svi`

### Top-level options
- `symbols` - Optional list of symbols to filter (e.g., `["AAPL", "MSFT"]`)
- `min_market_cap` - Optional minimum market cap filter
- `parallel` - Enable parallel processing (default: `true`)

## Creating Your Own Strategy

1. Copy one of the example configs as a starting point
2. Modify only the values you want to change
3. Use `--conf` to load your strategy config
4. Override specific values with CLI args as needed
