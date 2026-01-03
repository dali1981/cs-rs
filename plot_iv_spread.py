#!/usr/bin/env python3
"""Plot IV spread between nearest and 30-day IV to highlight earnings signals"""

import polars as pl
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta, date
import sys

if len(sys.argv) < 2:
    print("Usage: python plot_iv_spread.py <parquet_file>")
    sys.exit(1)

# Load data
df = pl.read_parquet(sys.argv[1])

# Convert date from days since epoch
epoch = datetime(1970, 1, 1)
df = df.with_columns([
    pl.col('date').map_elements(
        lambda d: epoch + timedelta(days=d),
        return_dtype=pl.Datetime
    ).alias('datetime')
])

# Calculate IV spread
df = df.with_columns([
    (pl.col('atm_iv_nearest') - pl.col('atm_iv_30d')).alias('iv_spread'),
    ((pl.col('atm_iv_nearest') - pl.col('atm_iv_30d')) / pl.col('atm_iv_30d') * 100).alias('iv_spread_pct')
])

# Convert to pandas for plotting
df_pd = df.to_pandas()

# Create figure with 3 subplots
fig, (ax1, ax2, ax3) = plt.subplots(3, 1, figsize=(14, 12))

# Plot 1: Nearest vs 30-day IV time series
ax1.plot(df_pd['datetime'], df_pd['atm_iv_nearest'] * 100,
         label='Nearest IV', linewidth=2, alpha=0.8, color='red', linestyle='--')
ax1.plot(df_pd['datetime'], df_pd['atm_iv_30d'] * 100,
         label='30-day IV', linewidth=2, alpha=0.8, color='blue')
ax1.set_ylabel('Implied Volatility (%)', fontsize=12)
ax1.set_title(f'{df["symbol"].unique()[0]} - Nearest vs 30-day IV Comparison',
              fontsize=14, fontweight='bold')
ax1.legend(fontsize=11)
ax1.grid(True, alpha=0.3)
ax1.xaxis.set_major_formatter(mdates.DateFormatter('%b %Y'))

# Plot 2: IV Spread (absolute)
ax2.fill_between(df_pd['datetime'], 0, df_pd['iv_spread'] * 100,
                 where=(df_pd['iv_spread'] > 0), alpha=0.5, color='red', label='Nearest > 30d')
ax2.fill_between(df_pd['datetime'], 0, df_pd['iv_spread'] * 100,
                 where=(df_pd['iv_spread'] <= 0), alpha=0.5, color='green', label='Nearest < 30d')
ax2.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
ax2.set_ylabel('IV Spread (pp)', fontsize=12)
ax2.set_title('IV Spread: Nearest - 30day (Earnings Signal)', fontsize=14, fontweight='bold')
ax2.legend(fontsize=11)
ax2.grid(True, alpha=0.3)
ax2.xaxis.set_major_formatter(mdates.DateFormatter('%b %Y'))

# Plot 3: IV Spread percentage
colors = ['red' if x > 20 else 'orange' if x > 10 else 'gray' for x in df_pd['iv_spread_pct']]
ax3.bar(df_pd['datetime'], df_pd['iv_spread_pct'], color=colors, alpha=0.7, width=1)
ax3.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
ax3.axhline(y=20, color='red', linestyle='--', linewidth=1, alpha=0.5, label='20% threshold')
ax3.set_ylabel('IV Spread (%)', fontsize=12)
ax3.set_xlabel('Date', fontsize=12)
ax3.set_title('IV Spread Percentage (Red bars = strong earnings signal)', fontsize=14, fontweight='bold')
ax3.legend(fontsize=11)
ax3.grid(True, alpha=0.3)
ax3.xaxis.set_major_formatter(mdates.DateFormatter('%b %Y'))

plt.tight_layout()

# Save plot
from pathlib import Path
input_path = Path(sys.argv[1])
output_file = input_path.parent / f"{input_path.stem}_iv_spread.png"
plt.savefig(output_file, dpi=300, bbox_inches='tight')
print(f"Saved plot to: {output_file}")
