#!/usr/bin/env python3
"""
Basic statistics and inspection of constant-maturity IV parquet output.

Usage:
    python plot_cm_basic_stats.py <parquet_file>
"""

import polars as pl
import sys
from datetime import datetime, timedelta

if len(sys.argv) < 2:
    print("Usage: python plot_cm_basic_stats.py <parquet_file>")
    sys.exit(1)

# Read the parquet file
df = pl.read_parquet(sys.argv[1])

# Convert date from days since epoch
epoch = datetime(1970, 1, 1)
df = df.with_columns([
    pl.col('date').map_elements(
        lambda d: epoch + timedelta(days=d),
        return_dtype=pl.Datetime
    ).alias('datetime')
])

# Convert to pandas for easier stats
df_pd = df.to_pandas()

print("\n" + "="*80)
print("CONSTANT-MATURITY IV DATA INSPECTION")
print("="*80)

# Show columns
print("\n--- Columns in Parquet File ---")
print(df.columns)

print(f"\n--- Data Coverage ---")
print(f"Total rows: {len(df)}")
print(f"Date range: {df_pd['datetime'].min().strftime('%Y-%m-%d')} to {df_pd['datetime'].max().strftime('%Y-%m-%d')}")

# Coverage stats
print(f"\n--- Field Coverage ---")
print(f"Rows with cm_iv_7d:  {df_pd['cm_iv_7d'].notna().sum()}/{len(df)} ({df_pd['cm_iv_7d'].notna().sum()/len(df)*100:.1f}%)")
print(f"Rows with cm_iv_14d: {df_pd['cm_iv_14d'].notna().sum()}/{len(df)} ({df_pd['cm_iv_14d'].notna().sum()/len(df)*100:.1f}%)")
print(f"Rows with cm_iv_21d: {df_pd['cm_iv_21d'].notna().sum()}/{len(df)} ({df_pd['cm_iv_21d'].notna().sum()/len(df)*100:.1f}%)")
print(f"Rows with cm_iv_30d: {df_pd['cm_iv_30d'].notna().sum()}/{len(df)} ({df_pd['cm_iv_30d'].notna().sum()/len(df)*100:.1f}%)")
print(f"Rows with cm_iv_60d: {df_pd['cm_iv_60d'].notna().sum()}/{len(df)} ({df_pd['cm_iv_60d'].notna().sum()/len(df)*100:.1f}%)")
print(f"Rows with cm_iv_90d: {df_pd['cm_iv_90d'].notna().sum()}/{len(df)} ({df_pd['cm_iv_90d'].notna().sum()/len(df)*100:.1f}%)")
print(f"\nRows with interpolated IVs: {df_pd['cm_interpolated'].sum()}/{len(df)} ({df_pd['cm_interpolated'].sum()/len(df)*100:.1f}%)")
print(f"Average # expirations used: {df_pd['cm_num_expirations'].mean():.1f}")

# Summary statistics
print("\n--- CM IV Summary Statistics ---")
print(f"{'Metric':<12} {'7d':>8} {'14d':>8} {'21d':>8} {'30d':>8} {'60d':>8} {'90d':>8}")
print("-" * 80)
print(f"{'Mean':<12} {df_pd['cm_iv_7d'].mean()*100:>7.2f}% {df_pd['cm_iv_14d'].mean()*100:>7.2f}% {df_pd['cm_iv_21d'].mean()*100:>7.2f}% {df_pd['cm_iv_30d'].mean()*100:>7.2f}% {df_pd['cm_iv_60d'].mean()*100:>7.2f}% {df_pd['cm_iv_90d'].mean()*100:>7.2f}%")
print(f"{'Median':<12} {df_pd['cm_iv_7d'].median()*100:>7.2f}% {df_pd['cm_iv_14d'].median()*100:>7.2f}% {df_pd['cm_iv_21d'].median()*100:>7.2f}% {df_pd['cm_iv_30d'].median()*100:>7.2f}% {df_pd['cm_iv_60d'].median()*100:>7.2f}% {df_pd['cm_iv_90d'].median()*100:>7.2f}%")
print(f"{'Min':<12} {df_pd['cm_iv_7d'].min()*100:>7.2f}% {df_pd['cm_iv_14d'].min()*100:>7.2f}% {df_pd['cm_iv_21d'].min()*100:>7.2f}% {df_pd['cm_iv_30d'].min()*100:>7.2f}% {df_pd['cm_iv_60d'].min()*100:>7.2f}% {df_pd['cm_iv_90d'].min()*100:>7.2f}%")
print(f"{'Max':<12} {df_pd['cm_iv_7d'].max()*100:>7.2f}% {df_pd['cm_iv_14d'].max()*100:>7.2f}% {df_pd['cm_iv_21d'].max()*100:>7.2f}% {df_pd['cm_iv_30d'].max()*100:>7.2f}% {df_pd['cm_iv_60d'].max()*100:>7.2f}% {df_pd['cm_iv_90d'].max()*100:>7.2f}%")
print(f"{'Std Dev':<12} {df_pd['cm_iv_7d'].std()*100:>7.2f}% {df_pd['cm_iv_14d'].std()*100:>7.2f}% {df_pd['cm_iv_21d'].std()*100:>7.2f}% {df_pd['cm_iv_30d'].std()*100:>7.2f}% {df_pd['cm_iv_60d'].std()*100:>7.2f}% {df_pd['cm_iv_90d'].std()*100:>7.2f}%")

# Term spreads
print("\n--- CM Term Spread Statistics ---")
print(f"Average 7d-30d spread:   {df_pd['cm_spread_7_30'].mean()*100:+.2f}pp")
print(f"Average 30d-60d spread:  {df_pd['cm_spread_30_60'].mean()*100:+.2f}pp")
print(f"Average 30d-90d spread:  {df_pd['cm_spread_30_90'].mean()*100:+.2f}pp")

# Sample data
print("\n--- Sample Data (first 5 rows) ---")
columns_to_show = [
    'datetime', 'spot',
    'cm_iv_7d', 'cm_iv_14d', 'cm_iv_30d',
    'cm_spread_7_30', 'cm_num_expirations'
]
print(df.select(columns_to_show).head(5))

print("\n" + "="*80)
