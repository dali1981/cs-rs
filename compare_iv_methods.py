#!/usr/bin/env python3
"""Compare EOD vs Minute-Aligned ATM IV computation methods."""

import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
import numpy as np

# Read data
eod = pl.read_parquet('./eod_full_year/atm_iv_AAPL.parquet')
minute = pl.read_parquet('./minute_aligned_full_year/atm_iv_AAPL.parquet')

# Convert dates from days since epoch to datetime
epoch = datetime(1970, 1, 1)
eod = eod.with_columns([
    pl.col('date').map_elements(lambda d: epoch + timedelta(days=d), return_dtype=pl.Datetime).alias('datetime')
])
minute = minute.with_columns([
    pl.col('date').map_elements(lambda d: epoch + timedelta(days=d), return_dtype=pl.Datetime).alias('datetime')
])

# Join datasets
joined = eod.join(minute, on='date', suffix='_minute')

# Calculate differences
joined = joined.with_columns([
    ((pl.col('atm_iv_30d_minute') - pl.col('atm_iv_30d')) * 100).alias('diff_30d_pct'),
    ((pl.col('atm_iv_60d_minute') - pl.col('atm_iv_60d')) * 100).alias('diff_60d_pct'),
    ((pl.col('atm_iv_90d_minute') - pl.col('atm_iv_90d')) * 100).alias('diff_90d_pct'),
])

# Convert to pandas for plotting
df = joined.to_pandas()

# Create figure with multiple subplots
fig = plt.figure(figsize=(16, 12))

# Subplot 1: Time series comparison - 30d IV
ax1 = plt.subplot(3, 2, 1)
ax1.plot(df['datetime'], df['atm_iv_30d'] * 100, label='EOD Method', alpha=0.7, linewidth=1.5)
ax1.plot(df['datetime'], df['atm_iv_30d_minute'] * 100, label='Minute-Aligned', alpha=0.7, linewidth=1.5)
ax1.set_ylabel('30-Day ATM IV (%)')
ax1.set_title('30-Day ATM IV: EOD vs Minute-Aligned')
ax1.legend()
ax1.grid(True, alpha=0.3)
ax1.xaxis.set_major_formatter(mdates.DateFormatter('%b'))

# Subplot 2: Difference over time - 30d
ax2 = plt.subplot(3, 2, 2)
ax2.plot(df['datetime'], df['diff_30d_pct'], color='red', alpha=0.7, linewidth=1)
ax2.axhline(y=0, color='black', linestyle='--', linewidth=0.8, alpha=0.5)
ax2.fill_between(df['datetime'], df['diff_30d_pct'], 0, alpha=0.3, color='red')
ax2.set_ylabel('Difference (%)')
ax2.set_title('30-Day IV Difference (Minute-Aligned - EOD)')
ax2.grid(True, alpha=0.3)
ax2.xaxis.set_major_formatter(mdates.DateFormatter('%b'))

# Subplot 3: 60d IV comparison
ax3 = plt.subplot(3, 2, 3)
mask_60d = df['atm_iv_60d'].notna() & df['atm_iv_60d_minute'].notna()
ax3.plot(df.loc[mask_60d, 'datetime'], df.loc[mask_60d, 'atm_iv_60d'] * 100,
         label='EOD Method', alpha=0.7, linewidth=1.5)
ax3.plot(df.loc[mask_60d, 'datetime'], df.loc[mask_60d, 'atm_iv_60d_minute'] * 100,
         label='Minute-Aligned', alpha=0.7, linewidth=1.5)
ax3.set_ylabel('60-Day ATM IV (%)')
ax3.set_title('60-Day ATM IV: EOD vs Minute-Aligned')
ax3.legend()
ax3.grid(True, alpha=0.3)
ax3.xaxis.set_major_formatter(mdates.DateFormatter('%b'))

# Subplot 4: Difference over time - 60d
ax4 = plt.subplot(3, 2, 4)
ax4.plot(df.loc[mask_60d, 'datetime'], df.loc[mask_60d, 'diff_60d_pct'],
         color='orange', alpha=0.7, linewidth=1)
ax4.axhline(y=0, color='black', linestyle='--', linewidth=0.8, alpha=0.5)
ax4.fill_between(df.loc[mask_60d, 'datetime'], df.loc[mask_60d, 'diff_60d_pct'], 0,
                  alpha=0.3, color='orange')
ax4.set_ylabel('Difference (%)')
ax4.set_title('60-Day IV Difference (Minute-Aligned - EOD)')
ax4.grid(True, alpha=0.3)
ax4.xaxis.set_major_formatter(mdates.DateFormatter('%b'))

# Subplot 5: Distribution of differences
ax5 = plt.subplot(3, 2, 5)
valid_diffs_30d = df['diff_30d_pct'].dropna()
valid_diffs_60d = df['diff_60d_pct'].dropna()
valid_diffs_90d = df['diff_90d_pct'].dropna()

ax5.hist(valid_diffs_30d, bins=50, alpha=0.6, label='30d IV', color='blue')
ax5.hist(valid_diffs_60d, bins=30, alpha=0.6, label='60d IV', color='orange')
ax5.hist(valid_diffs_90d, bins=30, alpha=0.6, label='90d IV', color='green')
ax5.axvline(x=0, color='black', linestyle='--', linewidth=1)
ax5.set_xlabel('Difference (%)')
ax5.set_ylabel('Frequency')
ax5.set_title('Distribution of IV Differences')
ax5.legend()
ax5.grid(True, alpha=0.3)

# Subplot 6: Scatter plot - correlation
ax6 = plt.subplot(3, 2, 6)
ax6.scatter(df['atm_iv_30d'] * 100, df['atm_iv_30d_minute'] * 100,
            alpha=0.5, s=20, label='30d IV')
ax6.plot([0, 50], [0, 50], 'k--', linewidth=1, alpha=0.5, label='Perfect Agreement')
ax6.set_xlabel('EOD Method IV (%)')
ax6.set_ylabel('Minute-Aligned Method IV (%)')
ax6.set_title('Correlation: EOD vs Minute-Aligned (30d)')
ax6.legend()
ax6.grid(True, alpha=0.3)

# Calculate correlation
corr = df['atm_iv_30d'].corr(df['atm_iv_30d_minute'])
ax6.text(0.05, 0.95, f'Correlation: {corr:.4f}',
         transform=ax6.transAxes, fontsize=10,
         verticalalignment='top',
         bbox=dict(boxstyle='round', facecolor='wheat', alpha=0.5))

plt.tight_layout()
plt.savefig('./iv_comparison_full_year.png', dpi=300, bbox_inches='tight')
print("Saved plot to: ./iv_comparison_full_year.png")

# Print statistics
print("\n" + "="*80)
print("STATISTICS: EOD vs Minute-Aligned IV Comparison (AAPL 2025)")
print("="*80)

print("\n30-Day IV:")
print(f"  Observations: {len(valid_diffs_30d)}")
print(f"  Mean difference: {valid_diffs_30d.mean():+.4f}%")
print(f"  Median difference: {valid_diffs_30d.median():+.4f}%")
print(f"  Std deviation: {valid_diffs_30d.std():.4f}%")
print(f"  Min difference: {valid_diffs_30d.min():+.4f}%")
print(f"  Max difference: {valid_diffs_30d.max():+.4f}%")
print(f"  % of days with |diff| > 1%: {(np.abs(valid_diffs_30d) > 1).sum() / len(valid_diffs_30d) * 100:.1f}%")
print(f"  % of days with |diff| > 2%: {(np.abs(valid_diffs_30d) > 2).sum() / len(valid_diffs_30d) * 100:.1f}%")

print("\n60-Day IV:")
print(f"  Observations: {len(valid_diffs_60d)}")
print(f"  Mean difference: {valid_diffs_60d.mean():+.4f}%")
print(f"  Median difference: {valid_diffs_60d.median():+.4f}%")
print(f"  Std deviation: {valid_diffs_60d.std():.4f}%")
print(f"  Min difference: {valid_diffs_60d.min():+.4f}%")
print(f"  Max difference: {valid_diffs_60d.max():+.4f}%")

print("\n90-Day IV:")
print(f"  Observations: {len(valid_diffs_90d)}")
print(f"  Mean difference: {valid_diffs_90d.mean():+.4f}%")
print(f"  Median difference: {valid_diffs_90d.median():+.4f}%")
print(f"  Std deviation: {valid_diffs_90d.std():.4f}%")
print(f"  Min difference: {valid_diffs_90d.min():+.4f}%")
print(f"  Max difference: {valid_diffs_90d.max():+.4f}%")

print("\nCorrelation (EOD vs Minute-Aligned):")
print(f"  30d IV: {df['atm_iv_30d'].corr(df['atm_iv_30d_minute']):.6f}")
print(f"  60d IV: {df['atm_iv_60d'].corr(df['atm_iv_60d_minute']):.6f}")
print(f"  90d IV: {df['atm_iv_90d'].corr(df['atm_iv_90d_minute']):.6f}")

print("\n" + "="*80)
