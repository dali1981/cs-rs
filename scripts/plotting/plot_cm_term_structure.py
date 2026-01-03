#!/usr/bin/env python3
"""
Plot constant-maturity IV term structure with earnings detection signals.

Usage:
    python plot_cm_term_structure.py <parquet_file> [output_file]
"""

import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import sys
from pathlib import Path

if len(sys.argv) < 2:
    print("Usage: python plot_cm_term_structure.py <parquet_file> [output_file]")
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

# Create figure with 3 subplots
fig, (ax1, ax2, ax3) = plt.subplots(3, 1, figsize=(14, 12))

# Plot 1: Constant-Maturity IV Term Structure
ax1.plot(df_pd['datetime'], df_pd['cm_iv_7d'] * 100,
         label='CM 7d IV', linewidth=2, marker='o', markersize=4, alpha=0.8, color='#e74c3c')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_14d'] * 100,
         label='CM 14d IV', linewidth=2, marker='s', markersize=4, alpha=0.8, color='#e67e22')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_21d'] * 100,
         label='CM 21d IV', linewidth=2, marker='^', markersize=4, alpha=0.8, color='#f39c12')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_30d'] * 100,
         label='CM 30d IV', linewidth=2, marker='d', markersize=4, alpha=0.8, color='#2ecc71')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_60d'] * 100,
         label='CM 60d IV', linewidth=2, marker='v', markersize=4, alpha=0.8, color='#3498db')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_90d'] * 100,
         label='CM 90d IV', linewidth=2, marker='<', markersize=4, alpha=0.8, color='#9b59b6')
ax1.set_ylabel('Implied Volatility (%)', fontsize=12)
ax1.set_title(f'{df["symbol"].unique()[0]} - Constant-Maturity IV Term Structure',
              fontsize=14, fontweight='bold')
ax1.legend(fontsize=10, ncol=3, loc='upper left')
ax1.grid(True, alpha=0.3)
ax1.xaxis.set_major_formatter(mdates.DateFormatter('%b %d'))

# Plot 2: CM Front-Week Spread (earnings signal)
spread_7_30 = df_pd['cm_spread_7_30'] * 100
ax2.fill_between(df_pd['datetime'], 0, spread_7_30,
                 where=(spread_7_30 > 0), alpha=0.5, color='red', label='7d > 30d (risk-on)')
ax2.fill_between(df_pd['datetime'], 0, spread_7_30,
                 where=(spread_7_30 <= 0), alpha=0.5, color='green', label='7d < 30d (normal)')
ax2.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
ax2.axhline(y=5, color='orange', linestyle='--', linewidth=1, alpha=0.5, label='5pp threshold')
ax2.axhline(y=10, color='red', linestyle='--', linewidth=1, alpha=0.5, label='10pp threshold')
ax2.set_ylabel('IV Spread (pp)', fontsize=12)
ax2.set_title('CM Front-Week Spread: 7d - 30d (Earnings Signal)', fontsize=14, fontweight='bold')
ax2.legend(fontsize=11)
ax2.grid(True, alpha=0.3)
ax2.xaxis.set_major_formatter(mdates.DateFormatter('%b %d'))

# Plot 3: CM Term Spreads (term structure shape)
ax3.plot(df_pd['datetime'], df_pd['cm_spread_30_60'] * 100,
         label='CM 30-60 Spread', linewidth=2, marker='o', markersize=4, alpha=0.8, color='blue')
ax3.plot(df_pd['datetime'], df_pd['cm_spread_30_90'] * 100,
         label='CM 30-90 Spread', linewidth=2, marker='s', markersize=4, alpha=0.8, color='orange')
ax3.axhline(y=0, color='black', linestyle='--', linewidth=0.5, alpha=0.5)
ax3.set_ylabel('IV Spread (pp)', fontsize=12)
ax3.set_xlabel('Date', fontsize=12)
ax3.set_title('CM Term Spreads (Positive = Backwardation)', fontsize=14, fontweight='bold')
ax3.legend(fontsize=11)
ax3.grid(True, alpha=0.3)
ax3.xaxis.set_major_formatter(mdates.DateFormatter('%b %d'))

plt.tight_layout()

# Save or show
if output_file:
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"Saved plot to: {output_file}")
else:
    # Auto-generate output filename
    input_path = Path(input_file)
    output_path = input_path.parent / f"{input_path.stem}_term_structure.png"
    plt.savefig(output_path, dpi=300, bbox_inches='tight')
    print(f"Saved plot to: {output_path}")

# Print some insights
print("\n=== Analysis Insights ===")
print(f"Average front-week spread (7d-30d): {df_pd['cm_spread_7_30'].mean()*100:.2f} pp")
print(f"Average 30-60 spread: {df_pd['cm_spread_30_60'].mean()*100:.2f} pp")
print(f"Average 30-90 spread: {df_pd['cm_spread_30_90'].mean()*100:.2f} pp")
print(f"\nAll IVs interpolated: {df_pd['cm_interpolated'].all()}")
print(f"Average # expirations used: {df_pd['cm_num_expirations'].mean():.1f}")

# Check for term structure shape
backwardation_days = (df_pd['cm_spread_30_60'] > 0).sum()
print(f"\nTerm structure in backwardation (30d > 60d): {backwardation_days}/{len(df_pd)} days")
