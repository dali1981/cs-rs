# Testing with finq Python Library

This document explains how to use the finq Python library for testing and data investigation in the cs-rs project.

## Setup

### 1. Install Dependencies

The finq library is configured as a local editable dependency in `pyproject.toml`:

```toml
[project]
dependencies = [
    "finq",
    "polars>=1.0.0",
    "pyarrow>=14.0.0",
]

[tool.uv.sources]
finq = { path = "/Users/mohamedali/polygon", editable = true }
```

Install with:
```bash
uv sync
```

### 2. Import and Initialize

```python
from finq import Finq
from datetime import date
import polars as pl

# Initialize client with data directory
client = Finq(data_dir="/Users/mohamedali/polygon/data")
```

## Available APIs

### Options Data

```python
# Get options bars for a specific date
options_list = client.options.bars(
    "AAPL",
    from_date=date(2025, 11, 10),
    to_date=date(2025, 11, 10),
    limit=100000  # High limit to get all options
)

# Convert to Polars DataFrame for analysis
options = pl.DataFrame([vars(bar) for bar in options_list])

# Columns available: symbol, strike, expiration, option_type,
#                    open, high, low, close, volume, open_interest, etc.
```

### Stock/Equity Data

```python
# Get stock bars
equity_list = client.stocks.bars(
    "AAPL",
    from_date=date(2025, 11, 10),
    to_date=date(2025, 11, 10)
)

if equity_list:
    spot_price = equity_list[0].close
    print(f"Spot: ${spot_price:.2f}")
```

### Other Available APIs

- `client.stocks` - Stock market data
- `client.options` - Options data
- `client.indices` - Index data
- `client.forex` - Forex data
- `client.crypto` - Cryptocurrency data
- `client.iv` - Implied volatility data
- `client.hv` - Historical volatility data
- `client.market` - General market data
- `client.stream` - Real-time streaming data

## Common Patterns

### Check if Options Data Exists

```python
def check_options_available(symbol: str, check_date: date) -> bool:
    """Check if options data is available for a symbol on a date."""
    try:
        options_list = client.options.bars(
            symbol,
            from_date=check_date,
            to_date=check_date,
            limit=10
        )
        return len(options_list) > 0
    except Exception as e:
        print(f"Error: {e}")
        return False
```

### Find Available Strikes for an Expiration

```python
def get_strikes_for_expiration(symbol: str, check_date: date, expiration: date):
    """Get all available strikes for a specific expiration."""
    options_list = client.options.bars(
        symbol,
        from_date=check_date,
        to_date=check_date,
        limit=100000
    )

    if not options_list:
        return []

    # Convert to DataFrame
    options = pl.DataFrame([vars(bar) for bar in options_list])

    # Filter to expiration and get unique strikes
    strikes = (
        options
        .filter(pl.col('expiration') == expiration)
        .select('strike')
        .unique()
        .sort('strike')
        ['strike']
        .to_list()
    )

    return strikes
```

### Investigate Pricing Failures

When the backtest reports pricing errors like:
```
"Cannot determine IV for put strike 2.5, expiration 2025-12-19 - no market data and interpolation failed"
```

Use finq to investigate:

```python
from finq import Finq
from datetime import date
import polars as pl

client = Finq(data_dir="/Users/mohamedali/polygon/data")

symbol = "PLBY"
check_date = date(2025, 11, 12)
target_expiration = date(2025, 12, 19)

# Check if options data exists
options_list = client.options.bars(
    symbol,
    from_date=check_date,
    to_date=check_date,
    limit=100000
)

print(f"Options bars found: {len(options_list)}")

if options_list:
    options = pl.DataFrame([vars(bar) for bar in options_list])

    # Show available expirations
    expirations = sorted(options['expiration'].unique().to_list())
    print(f"Available expirations: {expirations}")

    # Check specific expiration
    exp_data = options.filter(pl.col('expiration') == target_expiration)
    if len(exp_data) > 0:
        strikes = sorted(exp_data['strike'].unique().to_list())
        print(f"Strikes for {target_expiration}: {strikes}")
    else:
        print(f"No data for expiration {target_expiration}")
else:
    print(f"❌ No options data available for {symbol}")

    # Check if stock data exists
    equity_list = client.stocks.bars(
        symbol,
        from_date=check_date,
        to_date=check_date
    )
    if equity_list:
        print(f"✓ Stock data exists, spot: ${equity_list[0].close:.2f}")
        print("→ This symbol has equity data but NO options data (not optionable)")
```

## Example: Investigating Failed Straddles

See the investigation in this commit that found QRHC, CGEN, and PLBY don't have options data:

```python
uv run python3 << 'EOF'
from finq import Finq
from datetime import date
import polars as pl

client = Finq(data_dir="/Users/mohamedali/polygon/data")

# Symbols that failed pricing
symbols = [
    ("QRHC", date(2025, 11, 10)),
    ("CGEN", date(2025, 11, 10)),
    ("PLBY", date(2025, 11, 12))
]

for symbol, check_date in symbols:
    options_list = client.options.bars(
        symbol,
        from_date=check_date,
        to_date=check_date,
        limit=100000
    )

    equity_list = client.stocks.bars(
        symbol,
        from_date=check_date,
        to_date=check_date
    )

    print(f"{symbol}:")
    print(f"  Options: {len(options_list)} bars")
    if equity_list:
        print(f"  Spot: ${equity_list[0].close:.2f}")
    print()
EOF
```

Output:
```
QRHC:
  Options: 0 bars
  Spot: $1.41

CGEN:
  Options: 0 bars
  Spot: $1.67

PLBY:
  Options: 0 bars
  Spot: $1.36
```

**Conclusion**: These symbols have equity data but no options chains. They should be filtered earlier in the backtest process before attempting to price straddles.

## Reading Data Directly from Flatfiles

The finq Python API may not find data if the files are organized in a non-standard way. To investigate, read the parquet files directly:

### File Structure

```
~/polygon/data/flatfiles/
├── options/
│   ├── day_aggs/
│   │   └── 2025/
│   │       └── 2025-11-10/
│   │           ├── AAPL.parquet
│   │           ├── QRHC.parquet
│   │           └── ...
│   └── minute_aggs/
│       └── 2025/
│           └── ...
└── stocks/
    └── day_aggs/
        └── 2025/
            └── 2025-11-10/
                ├── AAPL.parquet
                └── ...
```

### Direct Read Example

```python
import polars as pl
from datetime import date

# Read options data directly
file_path = "/Users/mohamedali/polygon/data/flatfiles/options/day_aggs/2025/2025-11-10/QRHC.parquet"
df = pl.read_parquet(file_path)

print(f"Total options: {len(df)}")
print(f"Expirations: {sorted(df['expiration'].unique().to_list())}")
print(f"Strikes: {sorted(df['strike'].unique().to_list())}")
print(f"\nFull data:")
print(df)
```

### Real Investigation Example

The original investigation found that QRHC, CGEN, and PLBY had **sparse option chains** with missing strike/expiration combinations:

```python
uv run python3 << 'EOF'
import polars as pl
from datetime import date

file_path = "/Users/mohamedali/polygon/data/flatfiles/options/day_aggs/2025/2025-11-12/PLBY.parquet"
df = pl.read_parquet(file_path)

print("PLBY on 2025-11-12:")
print(df)

# Check for specific strike/expiration combo
dec_19_strike_2_5 = df.filter(
    (pl.col('expiration') == date(2025, 12, 19)) &
    (pl.col('strike') == 2.5)
)
print(f"\nDec 19 expiration + $2.50 strike: {len(dec_19_strike_2_5)} options")
EOF
```

**Output**:
```
PLBY on 2025-11-12:
shape: (6, 13)
┌─────────────────────┬────────┬───────┬────────────┬────────────┐
│ ticker              ┆ strike ┆ close ┆ expiration ┆ option_type│
├─────────────────────┼────────┼───────┼────────────┼────────────┤
│ O:PLBY251121C000025 ┆ 2.5    ┆ 0.03  ┆ 2025-11-21 ┆ call       │
│ O:PLBY251121P000025 ┆ 2.5    ┆ 1.15  ┆ 2025-11-21 ┆ put        │
│ O:PLBY251219C000050 ┆ 5.0    ┆ 0.2   ┆ 2025-12-19 ┆ call       │  ← Dec 19 exists!
│ O:PLBY260116C000025 ┆ 2.5    ┆ 0.05  ┆ 2026-01-16 ┆ call       │  ← Strike 2.5 exists!
│ O:PLBY260116C000050 ┆ 5.0    ┆ 0.05  ┆ 2026-01-16 ┆ call       │
│ O:PLBY260417C000025 ┆ 2.5    ┆ 0.12  ┆ 2026-04-17 ┆ call       │
└─────────────────────┴────────┴───────┴────────────┴────────────┘

Dec 19 expiration + $2.50 strike: 0 options  ← The combo doesn't exist!
```

**Conclusion**:
- Dec 19 expiration exists (but only with $5.00 strike)
- Strike $2.50 exists (but only on Nov 21, Jan 16, Apr 17 expirations)
- **The specific combination (Dec 19 + $2.50 strike) does NOT exist**
- This is expected for illiquid penny stocks with sparse option chains
- The pricing error is **correct** - you cannot price options that don't exist in the market

## Tips

1. **Use high limits**: Options data can have many strikes/expirations, set `limit=100000` to get all data
2. **Convert to DataFrames**: Polars/pandas make analysis easier than working with lists of objects
3. **Check data availability**: Many small-cap stocks don't have options despite having equity data
4. **Cache is automatic**: finq caches data locally, repeated calls are fast
5. **Use from_date/to_date**: These are keyword-only arguments

## API Signatures

```python
# Options
client.options.bars(
    symbol: str,
    *,
    days: int = 30,
    timeframe: str = 'day',
    multiplier: int = 1,
    from_date: date | str | None = None,
    to_date: date | str | None = None,
    limit: int = 100,
    use_cache: bool = True
) -> list[Bar]

# Stocks
client.stocks.bars(
    symbol: str,
    *,
    days: int = 30,
    timeframe: str = 'day',
    multiplier: int = 1,
    from_date: date | str | None = None,
    to_date: date | str | None = None,
    limit: int = 5000,
    use_cache: bool = True
) -> list[Bar]
```
