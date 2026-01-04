#!/usr/bin/env python3
"""
Plot constant-maturity IVs with trade entry/exit markers.

Usage:
    python plot_cm_with_trades.py <parquet_file> <output_file> --entry YYYY-MM-DD --exit YYYY-MM-DD [--earnings YYYY-MM-DD]
"""

import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import sys
from pathlib import Path
import argparse

parser = argparse.ArgumentParser(description='Plot constant-maturity IVs with trade markers')
parser.add_argument('parquet_file', help='Input parquet file')
parser.add_argument('output_file', help='Output PNG file')
parser.add_argument('--entry', required=True, help='Entry date (YYYY-MM-DD)')
parser.add_argument('--exit', required=True, help='Exit date (YYYY-MM-DD)')
parser.add_argument('--earnings', help='Earnings date (YYYY-MM-DD)')

args = parser.parse_args()

# Parse dates
entry_date = datetime.strptime(args.entry, '%Y-%m-%d')
exit_date = datetime.strptime(args.exit, '%Y-%m-%d')
earnings_date = datetime.strptime(args.earnings, '%Y-%m-%d') if args.earnings else None

# Read data
df = pl.read_parquet(args.parquet_file)

# Convert date from days since epoch
epoch = datetime(1970, 1, 1)
df = df.with_columns([
    pl.col('date').map_elements(
        lambda d: epoch + timedelta(days=d),
        return_dtype=pl.Datetime
    ).alias('datetime')
])

# Convert to pandas for plotting
df_pd = df.to_pandas()
symbol = df["symbol"].unique()[0]

# Create figure
fig, ax = plt.subplots(1, 1, figsize=(16, 8))

# Plot 7d, 14d, and 30d constant-maturity IVs
ax.plot(df_pd['datetime'], df_pd['cm_iv_7d'] * 100,
         label='7-Day IV', linewidth=2.5, alpha=0.9, color='#e74c3c')
ax.plot(df_pd['datetime'], df_pd['cm_iv_14d'] * 100,
         label='14-Day IV', linewidth=2.5, alpha=0.9, color='#3498db')
ax.plot(df_pd['datetime'], df_pd['cm_iv_30d'] * 100,
         label='30-Day IV', linewidth=2.5, alpha=0.9, color='#2ecc71')

# Add vertical lines for entry and exit
ax.axvline(x=entry_date, color='green', linestyle='--', linewidth=2.5,
           alpha=0.7, label=f'Entry: {entry_date.strftime("%b %d")}')
ax.axvline(x=exit_date, color='red', linestyle='--', linewidth=2.5,
           alpha=0.7, label=f'Exit: {exit_date.strftime("%b %d")}')

# Add earnings line if provided
if earnings_date:
    ax.axvline(x=earnings_date, color='purple', linestyle=':', linewidth=3,
               alpha=0.8, label=f'Earnings: {earnings_date.strftime("%b %d")}')

# Formatting
ax.set_ylabel('Implied Volatility (%)', fontsize=14, fontweight='bold')
ax.set_xlabel('Date', fontsize=14, fontweight='bold')
ax.set_title(f'{symbol} - Constant-Maturity IV Evolution Around Earnings\n7-Day, 14-Day, and 30-Day IVs',
             fontsize=16, fontweight='bold', pad=20)
ax.legend(fontsize=12, loc='upper left', framealpha=0.95)
ax.grid(True, alpha=0.3, linestyle='--')
ax.xaxis.set_major_formatter(mdates.DateFormatter('%b %d'))
ax.xaxis.set_major_locator(mdates.DayLocator(interval=2))
plt.setp(ax.xaxis.get_majorticklabels(), rotation=45, ha='right')

plt.tight_layout()

# Save
plt.savefig(args.output_file, dpi=300, bbox_inches='tight')
print(f"Saved plot to: {args.output_file}")

# Print IV at entry and exit dates
print("\n=== Trade Analysis ===")
entry_row = df_pd[df_pd['datetime'].dt.date == entry_date.date()]
exit_row = df_pd[df_pd['datetime'].dt.date == exit_date.date()]

if not entry_row.empty:
    print(f"\nEntry ({entry_date.strftime('%Y-%m-%d')} EOD):")
    print(f"  7d IV:  {entry_row['cm_iv_7d'].values[0]*100:.1f}%")
    print(f"  14d IV: {entry_row['cm_iv_14d'].values[0]*100:.1f}%")
    print(f"  30d IV: {entry_row['cm_iv_30d'].values[0]*100:.1f}%")
    print(f"  Spot:   ${entry_row['spot'].values[0]:.2f}")
else:
    print(f"\nNo data for entry date {entry_date.strftime('%Y-%m-%d')}")

if not exit_row.empty:
    print(f"\nExit ({exit_date.strftime('%Y-%m-%d')} EOD):")
    print(f"  7d IV:  {exit_row['cm_iv_7d'].values[0]*100:.1f}%")
    print(f"  14d IV: {exit_row['cm_iv_14d'].values[0]*100:.1f}%")
    print(f"  30d IV: {exit_row['cm_iv_30d'].values[0]*100:.1f}%")
    print(f"  Spot:   ${exit_row['spot'].values[0]:.2f}")
else:
    print(f"\nNo data for exit date {exit_date.strftime('%Y-%m-%d')}")

# Calculate IV change
if not entry_row.empty and not exit_row.empty:
    iv_change_7d = (exit_row['cm_iv_7d'].values[0] - entry_row['cm_iv_7d'].values[0]) * 100
    iv_change_30d = (exit_row['cm_iv_30d'].values[0] - entry_row['cm_iv_30d'].values[0]) * 100
    print(f"\nIV Change (Entry → Exit):")
    print(f"  7d IV:  {iv_change_7d:+.1f}pp")
    print(f"  30d IV: {iv_change_30d:+.1f}pp")
