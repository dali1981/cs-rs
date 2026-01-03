#!/usr/bin/env python3
"""
Simple plot of 7d, 14d, and 30d constant-maturity IVs.

Usage:
    python plot_cm_simple.py <parquet_file> [output_file]
"""

import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import sys
from pathlib import Path

if len(sys.argv) < 2:
    print("Usage: python plot_cm_simple.py <parquet_file> [output_file]")
    sys.exit(1)

input_file = sys.argv[1]
output_file = sys.argv[2] if len(sys.argv) > 2 else None

# Read data
df = pl.read_parquet(input_file)

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

# Formatting
ax.set_ylabel('Implied Volatility (%)', fontsize=14, fontweight='bold')
ax.set_xlabel('Date', fontsize=14, fontweight='bold')
ax.set_title(f'{symbol} - Constant-Maturity Implied Volatility\n7-Day, 14-Day, and 30-Day IVs',
             fontsize=16, fontweight='bold', pad=20)
ax.legend(fontsize=13, loc='upper left', framealpha=0.95)
ax.grid(True, alpha=0.3, linestyle='--')
ax.xaxis.set_major_formatter(mdates.DateFormatter('%b %Y'))
ax.xaxis.set_major_locator(mdates.MonthLocator())
plt.setp(ax.xaxis.get_majorticklabels(), rotation=45, ha='right')

# Add horizontal reference line at 30d average
avg_30d = df_pd['cm_iv_30d'].mean() * 100
ax.axhline(y=avg_30d, color='gray', linestyle=':', linewidth=1.5,
           alpha=0.5, label=f'Avg 30d IV: {avg_30d:.1f}%')

plt.tight_layout()

# Save or show
if output_file:
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"Saved plot to: {output_file}")
else:
    # Auto-generate output filename
    input_path = Path(input_file)
    output_path = input_path.parent / f"{input_path.stem}_simple_7_14_30.png"
    plt.savefig(output_path, dpi=300, bbox_inches='tight')
    print(f"Saved plot to: {output_path}")

# Print statistics
print("\n=== Statistics ===")
print(f"Data coverage: {len(df_pd)} observations")
print(f"Date range: {df_pd['datetime'].min().strftime('%Y-%m-%d')} to {df_pd['datetime'].max().strftime('%Y-%m-%d')}")

print("\n--- 7-Day IV ---")
print(f"  Mean:   {df_pd['cm_iv_7d'].mean()*100:.2f}%")
print(f"  Median: {df_pd['cm_iv_7d'].median()*100:.2f}%")
print(f"  Min:    {df_pd['cm_iv_7d'].min()*100:.2f}%")
print(f"  Max:    {df_pd['cm_iv_7d'].max()*100:.2f}%")

print("\n--- 14-Day IV ---")
print(f"  Mean:   {df_pd['cm_iv_14d'].mean()*100:.2f}%")
print(f"  Median: {df_pd['cm_iv_14d'].median()*100:.2f}%")
print(f"  Min:    {df_pd['cm_iv_14d'].min()*100:.2f}%")
print(f"  Max:    {df_pd['cm_iv_14d'].max()*100:.2f}%")

print("\n--- 30-Day IV ---")
print(f"  Mean:   {df_pd['cm_iv_30d'].mean()*100:.2f}%")
print(f"  Median: {df_pd['cm_iv_30d'].median()*100:.2f}%")
print(f"  Min:    {df_pd['cm_iv_30d'].min()*100:.2f}%")
print(f"  Max:    {df_pd['cm_iv_30d'].max()*100:.2f}%")

# Term structure
avg_spread_7_30 = (df_pd['cm_iv_7d'] - df_pd['cm_iv_30d']).mean() * 100
print("\n--- Term Structure ---")
print(f"  Average 7d-30d spread: {avg_spread_7_30:+.2f}pp")
if avg_spread_7_30 > 0:
    print("  → Short-term vol typically HIGHER (backwardation)")
else:
    print("  → Short-term vol typically LOWER (contango)")
