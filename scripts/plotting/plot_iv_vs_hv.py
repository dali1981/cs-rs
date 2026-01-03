#!/usr/bin/env python3
"""
Plot Implied Volatility vs Historical (Realized) Volatility.

Visualization panels:
1. IV Term Structure (7d, 14d, 30d CM IVs)
2. Historical Volatility (10d, 20d, 30d HV)
3. IV-HV Spread (Volatility Risk Premium)
4. IV/HV Ratio (how expensive options are vs realized)

Usage:
    python plot_iv_vs_hv.py <parquet_file> [--maturities 30,60] [--hv-window 30]
"""

import polars as pl
import matplotlib.pyplot as plt
from datetime import datetime, timedelta
import sys
from pathlib import Path
import argparse

def main():
    parser = argparse.ArgumentParser(description='Plot IV vs HV analysis')
    parser.add_argument('parquet_file', help='Input parquet file')
    parser.add_argument('--output', help='Output PNG file')
    parser.add_argument('--iv-tenors', default='7,14,30', help='IV tenors to plot (comma-separated)')
    parser.add_argument('--hv-windows', default='20,30', help='HV windows to plot (comma-separated)')
    args = parser.parse_args()

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

    # Create 4-panel figure
    fig, axes = plt.subplots(4, 1, figsize=(16, 16), sharex=True)

    # Panel 1: Implied Volatility
    ax1 = axes[0]
    colors_iv = {'7': '#e74c3c', '14': '#3498db', '21': '#f39c12', '30': '#2ecc71'}
    for tenor in args.iv_tenors.split(','):
        col = f'cm_iv_{tenor}d'
        if col in df_pd.columns and not df_pd[col].isna().all():
            color = colors_iv.get(tenor, 'gray')
            ax1.plot(df_pd['datetime'], df_pd[col] * 100,
                     label=f'{tenor}d IV', linewidth=2, color=color)

    ax1.set_ylabel('Implied Vol (%)', fontsize=12, fontweight='bold')
    ax1.set_title(f'{symbol} - Implied vs Realized Volatility Analysis', fontsize=14, fontweight='bold')
    ax1.legend(loc='upper left', fontsize=11)
    ax1.grid(True, alpha=0.3, linestyle='--')

    # Panel 2: Historical Volatility
    ax2 = axes[1]
    colors_hv = ['#e74c3c', '#3498db', '#2ecc71', '#9b59b6']
    for i, window in enumerate(args.hv_windows.split(',')):
        col = f'hv_{window}d'
        if col in df_pd.columns and not df_pd[col].isna().all():
            color = colors_hv[i % len(colors_hv)]
            ax2.plot(df_pd['datetime'], df_pd[col] * 100,
                     label=f'{window}d HV', linewidth=2, color=color)

    ax2.set_ylabel('Realized Vol (%)', fontsize=12, fontweight='bold')
    ax2.set_title('Historical (Realized) Volatility', fontsize=13, fontweight='bold')
    ax2.legend(loc='upper left', fontsize=11)
    ax2.grid(True, alpha=0.3, linestyle='--')

    # Panel 3: IV-HV Spread (Volatility Risk Premium)
    ax3 = axes[2]
    if 'iv_hv_spread_30d' in df_pd.columns and not df_pd['iv_hv_spread_30d'].isna().all():
        spread = df_pd['iv_hv_spread_30d'] * 100
        # Fill regions
        ax3.fill_between(df_pd['datetime'], 0, spread,
                         where=spread > 0, color='red', alpha=0.3, label='IV Premium (IV > HV)')
        ax3.fill_between(df_pd['datetime'], 0, spread,
                         where=spread <= 0, color='green', alpha=0.3, label='HV Premium (HV > IV)')
        ax3.plot(df_pd['datetime'], spread, color='black', linewidth=1.5)
        ax3.axhline(y=0, color='gray', linestyle='--', linewidth=1)

        # Add average line
        avg_spread = spread.mean()
        ax3.axhline(y=avg_spread, color='blue', linestyle=':', linewidth=2,
                    label=f'Mean: {avg_spread:+.2f}pp', alpha=0.7)

    ax3.set_ylabel('IV - HV (pp)', fontsize=12, fontweight='bold')
    ax3.set_title('Volatility Risk Premium (30d IV - 30d HV)', fontsize=13, fontweight='bold')
    ax3.legend(loc='upper left', fontsize=11)
    ax3.grid(True, alpha=0.3, linestyle='--')

    # Panel 4: IV/HV Ratio
    ax4 = axes[3]
    if ('cm_iv_30d' in df_pd.columns and 'hv_30d' in df_pd.columns and
        not df_pd['cm_iv_30d'].isna().all() and not df_pd['hv_30d'].isna().all()):
        # Compute ratio, avoiding division by zero
        ratio = df_pd['cm_iv_30d'] / df_pd['hv_30d'].replace(0, float('nan'))

        ax4.plot(df_pd['datetime'], ratio, color='purple', linewidth=2, label='IV/HV Ratio')
        ax4.axhline(y=1.0, color='gray', linestyle='--', linewidth=2, label='Fair Value (1.0)')

        # Fill regions
        ax4.fill_between(df_pd['datetime'], 1.0, ratio,
                         where=ratio > 1.0, color='red', alpha=0.2)
        ax4.fill_between(df_pd['datetime'], 1.0, ratio,
                         where=ratio <= 1.0, color='green', alpha=0.2)

        # Add average line
        avg_ratio = ratio.mean()
        ax4.axhline(y=avg_ratio, color='blue', linestyle=':', linewidth=2,
                    label=f'Mean: {avg_ratio:.2f}', alpha=0.7)

    ax4.set_ylabel('IV / HV Ratio', fontsize=12, fontweight='bold')
    ax4.set_xlabel('Date', fontsize=12, fontweight='bold')
    ax4.set_title('Relative IV (>1 = Options Expensive)', fontsize=13, fontweight='bold')
    ax4.legend(loc='upper left', fontsize=11)
    ax4.grid(True, alpha=0.3, linestyle='--')

    plt.tight_layout()

    # Save or show
    output_file = args.output or f"{Path(args.parquet_file).stem}_iv_vs_hv.png"
    plt.savefig(output_file, dpi=300, bbox_inches='tight')
    print(f"\n✓ Saved: {output_file}")

    # Print statistics
    print("\n" + "="*60)
    print("IV vs HV STATISTICS")
    print("="*60)

    # Data coverage
    print(f"\nData Coverage: {len(df_pd)} observations")
    print(f"Date range: {df_pd['datetime'].min().strftime('%Y-%m-%d')} to {df_pd['datetime'].max().strftime('%Y-%m-%d')}")

    # IV statistics
    if 'cm_iv_30d' in df_pd.columns:
        iv_30d = df_pd['cm_iv_30d'].dropna() * 100
        if len(iv_30d) > 0:
            print(f"\n30-Day Implied Volatility:")
            print(f"  Mean:   {iv_30d.mean():.2f}%")
            print(f"  Median: {iv_30d.median():.2f}%")
            print(f"  Min:    {iv_30d.min():.2f}%")
            print(f"  Max:    {iv_30d.max():.2f}%")

    # HV statistics
    if 'hv_30d' in df_pd.columns:
        hv_30d = df_pd['hv_30d'].dropna() * 100
        if len(hv_30d) > 0:
            print(f"\n30-Day Historical Volatility:")
            print(f"  Mean:   {hv_30d.mean():.2f}%")
            print(f"  Median: {hv_30d.median():.2f}%")
            print(f"  Min:    {hv_30d.min():.2f}%")
            print(f"  Max:    {hv_30d.max():.2f}%")

    # Spread statistics
    if 'iv_hv_spread_30d' in df_pd.columns:
        spread = df_pd['iv_hv_spread_30d'].dropna() * 100
        if len(spread) > 0:
            print(f"\n30d IV-HV Spread (Volatility Risk Premium):")
            print(f"  Mean:   {spread.mean():+.2f}pp")
            print(f"  Median: {spread.median():+.2f}pp")
            print(f"  Days IV > HV: {(spread > 0).sum()} / {len(spread)} ({100*(spread > 0).mean():.1f}%)")

            if spread.mean() > 0:
                print(f"  → Options typically EXPENSIVE vs realized vol")
            else:
                print(f"  → Options typically CHEAP vs realized vol")

    # Ratio statistics
    if 'cm_iv_30d' in df_pd.columns and 'hv_30d' in df_pd.columns:
        ratio = (df_pd['cm_iv_30d'] / df_pd['hv_30d'].replace(0, float('nan'))).dropna()
        if len(ratio) > 0:
            print(f"\nIV/HV Ratio:")
            print(f"  Mean:   {ratio.mean():.2f}x")
            print(f"  Median: {ratio.median():.2f}x")
            print(f"  Days IV/HV > 1: {(ratio > 1.0).sum()} / {len(ratio)} ({100*(ratio > 1.0).mean():.1f}%)")

if __name__ == '__main__':
    main()
