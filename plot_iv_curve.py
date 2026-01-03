#!/usr/bin/env python3
"""Plot IV curve (term structure) and time series"""

import polars as pl
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import sys

if len(sys.argv) < 2:
    print("Usage: python plot_iv_curve.py <parquet_file>")
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

# Convert to pandas
df_pd = df.to_pandas()

# Create figure with subplots
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10))

# Plot 1: IV Time Series
if 'atm_iv_nearest' in df_pd.columns and df_pd['atm_iv_nearest'].notna().any():
    ax1.plot(df_pd[df_pd['atm_iv_nearest'].notna()]['datetime'],
             df_pd[df_pd['atm_iv_nearest'].notna()]['atm_iv_nearest'] * 100,
             label='Nearest IV', linewidth=2, alpha=0.8, linestyle='--', color='red')
ax1.plot(df_pd['datetime'], df_pd['atm_iv_30d'] * 100,
         label='30-day IV', linewidth=2, alpha=0.8)
if df_pd['atm_iv_60d'].notna().any():
    ax1.plot(df_pd[df_pd['atm_iv_60d'].notna()]['datetime'],
             df_pd[df_pd['atm_iv_60d'].notna()]['atm_iv_60d'] * 100,
             label='60-day IV', linewidth=2, alpha=0.8)
if df_pd['atm_iv_90d'].notna().any():
    ax1.plot(df_pd[df_pd['atm_iv_90d'].notna()]['datetime'],
             df_pd[df_pd['atm_iv_90d'].notna()]['atm_iv_90d'] * 100,
             label='90-day IV', linewidth=2, alpha=0.8)

ax1.set_ylabel('Implied Volatility (%)', fontsize=12)
ax1.set_title(f'{df["symbol"].unique()[0]} - ATM Implied Volatility Time Series',
              fontsize=14, fontweight='bold')
ax1.legend(fontsize=11)
ax1.grid(True, alpha=0.3)
ax1.xaxis.set_major_formatter(mdates.DateFormatter('%b %Y'))
ax1.tick_params(axis='both', labelsize=10)

# Plot 2: Term Structure (latest date with all maturities)
# Find most recent date with all three IVs
complete_data = df_pd[
    df_pd['atm_iv_30d'].notna() &
    df_pd['atm_iv_60d'].notna() &
    df_pd['atm_iv_90d'].notna()
]

if len(complete_data) > 0:
    latest = complete_data.iloc[-1]

    maturities = []
    ivs = []

    # Add nearest IV if available
    if 'atm_iv_nearest' in df_pd.columns and pd.notna(latest.get('atm_iv_nearest')):
        if 'nearest_dte' in df_pd.columns and pd.notna(latest.get('nearest_dte')):
            maturities.append(int(latest['nearest_dte']))
            ivs.append(latest['atm_iv_nearest'] * 100)

    # Add standard maturities
    maturities.extend([30, 60, 90])
    ivs.extend([
        latest['atm_iv_30d'] * 100,
        latest['atm_iv_60d'] * 100,
        latest['atm_iv_90d'] * 100
    ])

    ax2.plot(maturities, ivs, 'o-', linewidth=2, markersize=8, color='darkblue')
    ax2.set_xlabel('Days to Expiration', fontsize=12)
    ax2.set_ylabel('Implied Volatility (%)', fontsize=12)
    ax2.set_title(f'IV Term Structure - {latest["datetime"].strftime("%Y-%m-%d")}',
                  fontsize=14, fontweight='bold')
    ax2.grid(True, alpha=0.3)
    ax2.tick_params(axis='both', labelsize=10)

    # Annotate points
    for i, (dte, iv) in enumerate(zip(maturities, ivs)):
        ax2.annotate(f'{iv:.1f}%',
                    (dte, iv),
                    textcoords="offset points",
                    xytext=(0,10),
                    ha='center',
                    fontsize=10,
                    fontweight='bold')
else:
    ax2.text(0.5, 0.5, 'No complete term structure data available',
             horizontalalignment='center',
             verticalalignment='center',
             transform=ax2.transAxes,
             fontsize=12)

plt.tight_layout()

# Save plot
from pathlib import Path
input_path = Path(sys.argv[1])
output_file = input_path.parent / f"{input_path.stem}_iv_curve.png"
plt.savefig(output_file, dpi=300, bbox_inches='tight')
print(f"Saved plot to: {output_file}")
