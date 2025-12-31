# cs-python: Python Bindings for Calendar Spread Backtest

PyO3 bindings exposing the Rust backtest engine to Python.

## Building

The Python bindings are built using [maturin](https://www.maturin.rs/):

```bash
# Install maturin (one-time setup)
pip install maturin

# Development build (installs in current Python environment)
cd cs-python
maturin develop --release

# Production wheel
maturin build --release
```

## Usage

### Analytics Functions

```python
from cs_rust import py_bs_price, py_bs_greeks, py_bs_implied_volatility

# Price an option
price = py_bs_price(
    spot=100.0,
    strike=100.0,
    time_to_expiry=0.25,  # 3 months
    volatility=0.20,      # 20% IV
    is_call=True,
    risk_free_rate=0.05
)
print(f"Option price: ${price:.2f}")

# Calculate Greeks
greeks = py_bs_greeks(
    spot=100.0,
    strike=100.0,
    time_to_expiry=0.25,
    volatility=0.20,
    is_call=True
)
print(f"Delta: {greeks.delta:.4f}")
print(f"Gamma: {greeks.gamma:.4f}")
print(f"Theta: {greeks.theta:.4f}")
print(f"Vega: {greeks.vega:.4f}")

# Calculate implied volatility
iv = py_bs_implied_volatility(
    option_price=5.50,
    spot=100.0,
    strike=100.0,
    time_to_expiry=0.25,
    is_call=True
)
print(f"Implied volatility: {iv:.2%}")
```

### Running Backtests

```python
from cs_rust import PyBacktestConfig, PyBacktestUseCase

# Configure backtest
config = PyBacktestConfig(
    data_dir="/path/to/your/data",
    entry_hour=9,
    entry_minute=35,
    exit_hour=15,
    exit_minute=55,
    min_short_dte=0,
    min_long_dte=7,
    min_iv_ratio=1.05,  # Long IV must be >= 5% higher than short
    parallel=True
)

# Create backtest instance
backtest = PyBacktestUseCase(config)

# Run backtest
result = backtest.execute(
    start_date="2024-01-01",
    end_date="2024-01-31",
    option_type="call"
)

# Analyze results
print(f"Sessions processed: {result.sessions_processed}")
print(f"Total trades: {result.total_entries}")
print(f"Win rate: {result.win_rate():.2%}")
print(f"Total P&L: ${result.total_pnl():.2f}")
print(f"Average P&L: ${result.avg_pnl():.2f}")

# Access individual trades
for trade in result.results[:5]:
    print(f"{trade.symbol} @ {trade.strike}: P&L ${trade.pnl:.2f} ({trade.pnl_pct:.2f}%)")
```

### Converting to Pandas

```python
import pandas as pd

# Convert results to DataFrame
trades_data = [
    {
        'symbol': t.symbol,
        'earnings_date': t.earnings_date,
        'strike': t.strike,
        'entry_cost': t.entry_cost,
        'exit_value': t.exit_value,
        'pnl': t.pnl,
        'pnl_pct': t.pnl_pct,
        'short_delta': t.short_delta,
        'long_delta': t.long_delta,
        'iv_ratio': t.iv_ratio(),
        'spot_at_entry': t.spot_at_entry,
        'spot_at_exit': t.spot_at_exit,
    }
    for t in result.results
]

df = pd.DataFrame(trades_data)
print(df.describe())
```

## API Reference

### Analytics Module

- **`py_bs_price(spot, strike, time_to_expiry, volatility, is_call, risk_free_rate=0.05)`**
  Calculate option price using Black-Scholes

- **`py_bs_implied_volatility(option_price, spot, strike, time_to_expiry, is_call)`**
  Calculate implied volatility from market price

- **`py_bs_greeks(spot, strike, time_to_expiry, volatility, is_call, risk_free_rate=0.05)`**
  Calculate option Greeks (delta, gamma, theta, vega, rho)

### Domain Types

- **`PyGreeks`**: Greeks container with `delta`, `gamma`, `theta`, `vega`, `rho` attributes

- **`PyCalendarSpreadResult`**: Individual trade result with:
  - Trade details: `symbol`, `strike`, `option_type`, expirations
  - Pricing: entry/exit prices, costs, P&L
  - Greeks: entry Greeks for both legs
  - IV data: entry/exit IV for both legs
  - P&L attribution: `delta_pnl`, `gamma_pnl`, `theta_pnl`, `vega_pnl`
  - Methods: `is_winner()`, `iv_ratio()`

### Backtest Module

- **`PyBacktestConfig`**: Configuration object with:
  - `data_dir`: Path to market data
  - `entry_hour`, `entry_minute`: Entry time
  - `exit_hour`, `exit_minute`: Exit time
  - `min_short_dte`, `min_long_dte`: DTE filters
  - `target_delta`: Target delta for strike selection
  - `min_iv_ratio`: Minimum long/short IV ratio
  - `symbols`: Optional list of symbols to trade
  - `min_market_cap`: Optional market cap filter
  - `parallel`: Enable parallel processing

- **`PyBacktestUseCase`**: Main backtest executor
  - `execute(start_date, end_date, option_type)`: Run backtest

- **`PyBacktestResult`**: Backtest results with:
  - `results`: List of `PyCalendarSpreadResult` objects
  - `sessions_processed`: Number of trading days processed
  - `total_entries`: Number of trades entered
  - `total_opportunities`: Total opportunities before filtering
  - Methods: `win_rate()`, `total_pnl()`, `avg_pnl()`

## Performance

The Rust implementation provides significant performance improvements over pure Python:

- **Black-Scholes calculations**: ~10-20x faster
- **Backtest execution**: ~5-10x faster with parallel processing
- **Memory efficiency**: Lower memory footprint for large backtests

## Requirements

- Python 3.8+
- Data directory with:
  - Earnings data (Parquet format)
  - Options chain data (finq-flatfiles format)
  - Equity price data (finq-flatfiles format)

## Development

```bash
# Run tests
cargo test --package cs-python

# Check compilation
cargo check --package cs-python

# Format code
cargo fmt --package cs-python
```
