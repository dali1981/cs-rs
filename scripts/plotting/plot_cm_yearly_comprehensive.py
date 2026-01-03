#!/usr/bin/env python3
"""
Comprehensive 4-panel constant-maturity IV analysis for a full year.

Usage:
    python plot_cm_yearly_comprehensive.py <parquet_file> [output_file]
"""

import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import sys
from pathlib import Path

if len(sys.argv) < 2:
    print("Usage: python plot_cm_yearly_comprehensive.py <parquet_file> [output_file]")
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

# Create comprehensive 4-panel figure
fig = plt.figure(figsize=(20, 14))
gs = fig.add_gridspec(4, 1, hspace=0.3)

# Panel 1: All constant-maturity IVs (7, 14, 21, 30, 60, 90 day)
ax1 = fig.add_subplot(gs[0])
ax1.plot(df_pd['datetime'], df_pd['cm_iv_7d'] * 100, label='7d IV', linewidth=2, alpha=0.9, color='#e74c3c')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_14d'] * 100, label='14d IV', linewidth=2, alpha=0.9, color='#e67e22')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_21d'] * 100, label='21d IV', linewidth=2, alpha=0.9, color='#f39c12')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_30d'] * 100, label='30d IV', linewidth=2.5, alpha=0.9, color='#2ecc71')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_60d'] * 100, label='60d IV', linewidth=2, alpha=0.8, color='#3498db')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_90d'] * 100, label='90d IV', linewidth=2, alpha=0.8, color='#9b59b6')
ax1.set_ylabel('IV (%)', fontsize=12, fontweight='bold')
ax1.set_title(f'{symbol} Constant-Maturity Implied Volatility - Full Year\nComplete Term Structure (7d, 14d, 21d, 30d, 60d, 90d)',
              fontsize=14, fontweight='bold', pad=10)
ax1.legend(fontsize=10, loc='upper left', ncol=6, framealpha=0.95)
ax1.grid(True, alpha=0.3, linestyle='--')
ax1.set_ylim(bottom=0)

# Panel 2: Focus on short-term IVs (7d, 14d, 30d)
ax2 = fig.add_subplot(gs[1])
ax2.plot(df_pd['datetime'], df_pd['cm_iv_7d'] * 100, label='7-Day IV', linewidth=2.5, alpha=0.9, color='#e74c3c', marker='o', markersize=2)
ax2.plot(df_pd['datetime'], df_pd['cm_iv_14d'] * 100, label='14-Day IV', linewidth=2.5, alpha=0.9, color='#3498db', marker='s', markersize=2)
ax2.plot(df_pd['datetime'], df_pd['cm_iv_30d'] * 100, label='30-Day IV', linewidth=2.5, alpha=0.9, color='#2ecc71', marker='^', markersize=2)
ax2.set_ylabel('IV (%)', fontsize=12, fontweight='bold')
ax2.set_title('Short-Term IV Focus (7d, 14d, 30d)', fontsize=13, fontweight='bold')
ax2.legend(fontsize=11, loc='upper left', framealpha=0.95)
ax2.grid(True, alpha=0.3, linestyle='--')

# Panel 3: Earnings detection (7d-30d spread)
ax3 = fig.add_subplot(gs[2])
spread_7_30 = df_pd['cm_spread_7_30'] * 100
ax3.fill_between(df_pd['datetime'], 0, spread_7_30, where=(spread_7_30 > 5), alpha=0.6, color='red', label='Strong signal (>5pp)')
ax3.fill_between(df_pd['datetime'], 0, spread_7_30, where=(spread_7_30 > 0) & (spread_7_30 <= 5), alpha=0.4, color='orange', label='Moderate signal (0-5pp)')
ax3.fill_between(df_pd['datetime'], spread_7_30, 0, where=(spread_7_30 < 0), alpha=0.3, color='green', label='Normal (<0pp)')
ax3.plot(df_pd['datetime'], spread_7_30, linewidth=1.5, alpha=0.9, color='darkblue')
ax3.axhline(y=0, color='black', linestyle='-', linewidth=1)
ax3.axhline(y=5, color='orange', linestyle='--', linewidth=1, alpha=0.5)
ax3.axhline(y=10, color='red', linestyle='--', linewidth=1, alpha=0.5)
ax3.set_ylabel('Spread (pp)', fontsize=12, fontweight='bold')
ax3.set_title('Earnings Detection Signal: 7d - 30d IV Spread', fontsize=13, fontweight='bold')
ax3.legend(fontsize=10, loc='upper left', framealpha=0.95)
ax3.grid(True, alpha=0.3, linestyle='--')

# Panel 4: Term structure spreads
ax4 = fig.add_subplot(gs[3])
spread_30_60 = df_pd['cm_spread_30_60'] * 100
spread_30_90 = df_pd['cm_spread_30_90'] * 100
ax4.plot(df_pd['datetime'], spread_30_60, label='30d-60d Spread', linewidth=2, alpha=0.8, color='#3498db', marker='o', markersize=2)
ax4.plot(df_pd['datetime'], spread_30_90, label='30d-90d Spread', linewidth=2, alpha=0.8, color='#9b59b6', marker='s', markersize=2)
ax4.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
ax4.fill_between(df_pd['datetime'], 0, 10, where=(spread_30_60 > 0), alpha=0.1, color='red', label='Backwardation')
ax4.fill_between(df_pd['datetime'], -10, 0, where=(spread_30_60 < 0), alpha=0.1, color='blue', label='Contango')
ax4.set_ylabel('Spread (pp)', fontsize=12, fontweight='bold')
ax4.set_xlabel('Date', fontsize=13, fontweight='bold')
ax4.set_title('Term Structure Shape: 30d-60d and 30d-90d Spreads', fontsize=13, fontweight='bold')
ax4.legend(fontsize=10, loc='upper left', framealpha=0.95)
ax4.grid(True, alpha=0.3, linestyle='--')

# Format x-axis for all panels
for ax in [ax1, ax2, ax3, ax4]:
    ax.xaxis.set_major_formatter(mdates.DateFormatter('%b'))
    ax.xaxis.set_major_locator(mdates.MonthLocator())
    ax.set_xlim(df_pd['datetime'].min(), df_pd['datetime'].max())

plt.setp(ax4.xaxis.get_majorticklabels(), rotation=0, ha='center', fontsize=11)

plt.tight_layout()

# Save or show
if output_file:
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"✓ Saved comprehensive plot to: {output_file}")
else:
    # Auto-generate output filename
    input_path = Path(input_file)
    output_path = input_path.parent / f"{input_path.stem}_comprehensive_analysis.png"
    plt.savefig(output_path, dpi=300, bbox_inches='tight')
    print(f"✓ Saved comprehensive plot to: {output_path}")

# Print summary
print("\n" + "="*90)
print(f"{symbol} CONSTANT-MATURITY IV - COMPREHENSIVE ANALYSIS")
print("="*90)
print(f"\nData Coverage: {len(df_pd)} observations from {df_pd['datetime'].min().strftime('%Y-%m-%d')} to {df_pd['datetime'].max().strftime('%Y-%m-%d')}")

print("\n--- VOLATILITY LEVELS (Average) ---")
print(f"  7-Day IV:   {df_pd['cm_iv_7d'].mean()*100:5.2f}%")
print(f"  14-Day IV:  {df_pd['cm_iv_14d'].mean()*100:5.2f}%")
print(f"  21-Day IV:  {df_pd['cm_iv_21d'].mean()*100:5.2f}%")
print(f"  30-Day IV:  {df_pd['cm_iv_30d'].mean()*100:5.2f}%")
print(f"  60-Day IV:  {df_pd['cm_iv_60d'].mean()*100:5.2f}%")
print(f"  90-Day IV:  {df_pd['cm_iv_90d'].mean()*100:5.2f}%")

print("\n--- TERM STRUCTURE (Average Spreads) ---")
print(f"  7d-30d:   {(df_pd['cm_iv_7d'] - df_pd['cm_iv_30d']).mean()*100:+6.2f}pp")
print(f"  14d-30d:  {(df_pd['cm_iv_14d'] - df_pd['cm_iv_30d']).mean()*100:+6.2f}pp")
print(f"  30d-60d:  {df_pd['cm_spread_30_60'].mean()*100:+6.2f}pp")
print(f"  30d-90d:  {df_pd['cm_spread_30_90'].mean()*100:+6.2f}pp")

print("\n--- VOLATILITY EXTREMES ---")
print(f"  Highest 7d IV:  {df_pd['cm_iv_7d'].max()*100:.2f}% on {df_pd.loc[df_pd['cm_iv_7d'].idxmax(), 'datetime'].strftime('%Y-%m-%d')}")
print(f"  Lowest 7d IV:   {df_pd['cm_iv_7d'].min()*100:.2f}% on {df_pd.loc[df_pd['cm_iv_7d'].idxmin(), 'datetime'].strftime('%Y-%m-%d')}")
print(f"  Highest 30d IV: {df_pd['cm_iv_30d'].max()*100:.2f}% on {df_pd.loc[df_pd['cm_iv_30d'].idxmax(), 'datetime'].strftime('%Y-%m-%d')}")
print(f"  Lowest 30d IV:  {df_pd['cm_iv_30d'].min()*100:.2f}% on {df_pd.loc[df_pd['cm_iv_30d'].idxmin(), 'datetime'].strftime('%Y-%m-%d')}")

# Earnings detection
strong_signals = df_pd[df_pd['cm_spread_7_30'] > 0.10]
print(f"\n--- EARNINGS SIGNALS ---")
print(f"  Days with strong signal (>10pp): {len(strong_signals)}")
print(f"  Max front-week spread: {df_pd['cm_spread_7_30'].max()*100:.2f}pp")

print("\n" + "="*90)
