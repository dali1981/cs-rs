#!/usr/bin/env python3
"""
Plot EOD IV vs actual intraday trade entry/exit IVs.

This shows the disconnect between EOD snapshots and intraday pricing.
"""

import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import argparse

parser = argparse.ArgumentParser(description='Plot EOD vs intraday IVs')
parser.add_argument('--eod-parquet', required=True, help='EOD IV parquet file')
parser.add_argument('--entry-date', required=True, help='Entry date (YYYY-MM-DD)')
parser.add_argument('--entry-iv', required=True, type=float, help='Entry IV (e.g., 0.95 for 95%)')
parser.add_argument('--exit-date', required=True, help='Exit date (YYYY-MM-DD)')
parser.add_argument('--exit-iv', required=True, type=float, help='Exit IV (e.g., 0.826 for 82.6%)')
parser.add_argument('--earnings-date', required=True, help='Earnings date (YYYY-MM-DD)')
parser.add_argument('--output', required=True, help='Output PNG file')

args = parser.parse_args()

# Parse dates
entry_date = datetime.strptime(args.entry_date, '%Y-%m-%d')
exit_date = datetime.strptime(args.exit_date, '%Y-%m-%d')
earnings_date = datetime.strptime(args.earnings_date, '%Y-%m-%d')

# Read EOD data
df = pl.read_parquet(args.eod_parquet)

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

# Create figure with two subplots
fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(16, 12), height_ratios=[3, 1])

# ===== TOP PLOT: IV Evolution =====

# Plot EOD IV curves
ax1.plot(df_pd['datetime'], df_pd['cm_iv_7d'] * 100,
         label='7-Day IV (EOD)', linewidth=2.5, alpha=0.7, color='#e74c3c', linestyle='-')
ax1.plot(df_pd['datetime'], df_pd['cm_iv_30d'] * 100,
         label='30-Day IV (EOD)', linewidth=2.5, alpha=0.7, color='#2ecc71', linestyle='-')

# Add entry/exit points as markers
ax1.scatter([entry_date], [args.entry_iv * 100],
           s=400, marker='o', color='darkgreen', edgecolors='white', linewidths=2,
           label=f'Entry (14:35): {args.entry_iv*100:.1f}%', zorder=5)
ax1.scatter([exit_date], [args.exit_iv * 100],
           s=400, marker='s', color='darkred', edgecolors='white', linewidths=2,
           label=f'Exit (20:55): {args.exit_iv*100:.1f}%', zorder=5)

# Add vertical lines
ax1.axvline(x=entry_date, color='green', linestyle='--', linewidth=1.5,
           alpha=0.5)
ax1.axvline(x=exit_date, color='red', linestyle='--', linewidth=1.5,
           alpha=0.5)
ax1.axvline(x=earnings_date, color='purple', linestyle=':', linewidth=2,
           alpha=0.6, label=f'Earnings: {earnings_date.strftime("%b %d")}')

# Get EOD IVs for comparison
entry_eod = df_pd[df_pd['datetime'].dt.date == entry_date.date()]
exit_eod = df_pd[df_pd['datetime'].dt.date == exit_date.date()]

if not entry_eod.empty:
    entry_eod_iv = entry_eod['cm_iv_7d'].values[0]
    ax1.scatter([entry_date], [entry_eod_iv * 100],
               s=200, marker='x', color='green', linewidths=3,
               label=f'Entry EOD: {entry_eod_iv*100:.1f}%', zorder=4)

if not exit_eod.empty:
    exit_eod_iv = exit_eod['cm_iv_7d'].values[0]
    ax1.scatter([exit_date], [exit_eod_iv * 100],
               s=200, marker='x', color='red', linewidths=3,
               label=f'Exit EOD: {exit_eod_iv*100:.1f}%', zorder=4)

# Formatting
ax1.set_ylabel('Implied Volatility (%)', fontsize=14, fontweight='bold')
ax1.set_title(f'{symbol} - EOD IV vs Intraday Trade IVs\n"Why EOD Data Misses Intraday Dynamics"',
             fontsize=16, fontweight='bold', pad=20)
ax1.legend(fontsize=11, loc='upper left', framealpha=0.95, ncol=2)
ax1.grid(True, alpha=0.3, linestyle='--')
ax1.xaxis.set_major_formatter(mdates.DateFormatter('%b %d'))
ax1.xaxis.set_major_locator(mdates.DayLocator(interval=2))
plt.setp(ax1.xaxis.get_majorticklabels(), rotation=45, ha='right')

# ===== BOTTOM PLOT: IV Difference =====

# Show the entry/exit disconnect
trade_dates = [entry_date, exit_date]
intraday_ivs = [args.entry_iv * 100, args.exit_iv * 100]

if not entry_eod.empty and not exit_eod.empty:
    eod_ivs = [entry_eod_iv * 100, exit_eod_iv * 100]

    # Plot comparison
    x_pos = [0, 1]
    width = 0.35

    ax2.bar([p - width/2 for p in x_pos], eod_ivs, width,
           label='EOD IV (16:00)', color='#3498db', alpha=0.8)
    ax2.bar([p + width/2 for p in x_pos], intraday_ivs, width,
           label='Actual Trade IV', color='#e74c3c', alpha=0.8)

    # Add value labels
    for i, (eod, intra) in enumerate(zip(eod_ivs, intraday_ivs)):
        ax2.text(i - width/2, eod + 2, f'{eod:.1f}%', ha='center', fontsize=11, fontweight='bold')
        ax2.text(i + width/2, intra + 2, f'{intra:.1f}%', ha='center', fontsize=11, fontweight='bold')

        # Show difference
        diff = intra - eod
        y_pos = max(eod, intra) + 5
        ax2.text(i, y_pos, f'Δ {diff:+.1f}pp', ha='center', fontsize=10,
                style='italic', color='red' if diff > 0 else 'green')

    ax2.set_xticks(x_pos)
    ax2.set_xticklabels(['Entry\n(Dec 1)', 'Exit\n(Dec 5)'])
    ax2.set_ylabel('IV (%)', fontsize=12, fontweight='bold')
    ax2.set_title('EOD vs Intraday IV Comparison', fontsize=14, fontweight='bold')
    ax2.legend(fontsize=11)
    ax2.grid(True, alpha=0.3, axis='y')

    # Calculate P&L impact
    eod_change = eod_ivs[1] - eod_ivs[0]
    intraday_change = intraday_ivs[1] - intraday_ivs[0]

    # Add text box with summary
    summary = f'EOD View: IV increased {eod_change:+.1f}pp (bullish for long vol)\n'
    summary += f'Reality: IV decreased {intraday_change:+.1f}pp (bearish for long vol)\n'
    summary += f'Timing Impact: {abs(eod_change - intraday_change):.1f}pp difference'

    ax2.text(0.98, 0.97, summary, transform=ax2.transAxes,
            fontsize=11, verticalalignment='top', horizontalalignment='right',
            bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.8),
            family='monospace')

plt.tight_layout()

# Save
plt.savefig(args.output, dpi=300, bbox_inches='tight')
print(f"Saved plot to: {args.output}")

# Print analysis
print("\n=== Intraday vs EOD Analysis ===")
print(f"\nEntry ({entry_date.strftime('%Y-%m-%d')}):")
if not entry_eod.empty:
    print(f"  EOD IV (16:00):     {entry_eod_iv*100:.1f}%")
print(f"  Actual IV (14:35):  {args.entry_iv*100:.1f}%")
if not entry_eod.empty:
    print(f"  Difference:         {(args.entry_iv - entry_eod_iv)*100:+.1f}pp")

print(f"\nExit ({exit_date.strftime('%Y-%m-%d')}):")
if not exit_eod.empty:
    print(f"  EOD IV (16:00):     {exit_eod_iv*100:.1f}%")
print(f"  Actual IV (20:55):  {args.exit_iv*100:.1f}%")
if not exit_eod.empty:
    print(f"  Difference:         {(args.exit_iv - exit_eod_iv)*100:+.1f}pp")

print(f"\nIV Change During Trade:")
if not entry_eod.empty and not exit_eod.empty:
    print(f"  EOD view:    {(exit_eod_iv - entry_eod_iv)*100:+.1f}pp")
print(f"  Actual:      {(args.exit_iv - args.entry_iv)*100:+.1f}pp")
if not entry_eod.empty and not exit_eod.empty:
    print(f"  \n  → EOD data would suggest {'+profit' if eod_change > 0 else 'loss'}")
    print(f"  → Reality was {'+profit' if intraday_change > 0 else 'loss'}")
