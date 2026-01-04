#!/usr/bin/env python3
"""
Compute intraday mean ATM IV from minute-level option data.

This shows the average IV throughout each trading day, not just EOD snapshot.
"""

import polars as pl
import numpy as np
from pathlib import Path
from datetime import datetime, timedelta
import argparse
from typing import Optional

# Black-Scholes IV calculation (simplified - using scipy)
try:
    from scipy.optimize import brentq
    from scipy.stats import norm
    SCIPY_AVAILABLE = True
except ImportError:
    print("Warning: scipy not available, will use approximate IV")
    SCIPY_AVAILABLE = False

def bs_call_price(S, K, T, r, sigma):
    """Black-Scholes call price"""
    if T <= 0 or sigma <= 0:
        return max(S - K, 0)

    d1 = (np.log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * np.sqrt(T))
    d2 = d1 - sigma * np.sqrt(T)
    return S * norm.cdf(d1) - K * np.exp(-r * T) * norm.cdf(d2)

def bs_put_price(S, K, T, r, sigma):
    """Black-Scholes put price"""
    if T <= 0 or sigma <= 0:
        return max(K - S, 0)

    d1 = (np.log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * np.sqrt(T))
    d2 = d1 - sigma * np.sqrt(T)
    return K * np.exp(-r * T) * norm.cdf(-d2) - S * norm.cdf(-d1)

def implied_volatility(price, S, K, T, r, option_type='call'):
    """Compute implied volatility using Brent's method"""
    if not SCIPY_AVAILABLE:
        # Rough approximation: IV ≈ price / (S * sqrt(T)) * sqrt(2*pi)
        return (price / S) * np.sqrt(2 * np.pi / T) if T > 0 else None

    if T <= 0:
        return None

    price_func = bs_call_price if option_type == 'call' else bs_put_price

    def objective(sigma):
        return price_func(S, K, T, r, sigma) - price

    try:
        iv = brentq(objective, 1e-6, 5.0, maxiter=100)
        return iv if 0.01 < iv < 3.0 else None
    except (ValueError, RuntimeError):
        return None

def compute_daily_mean_iv(
    symbol: str,
    start_date: str,
    end_date: str,
    data_dir: Path,
) -> pl.DataFrame:
    """
    Compute daily mean ATM IV from minute bars.

    For each day:
    1. Get all minute bars for ATM options (strike closest to spot)
    2. For each minute bar, compute IV using spot at that minute
    3. Average all IVs to get daily mean
    """

    print(f"Computing intraday mean IV for {symbol}...")
    print(f"Data dir: {data_dir}")

    # Load minute bars for the period
    options_dir = data_dir / "options_minute_bars" / symbol
    equity_file = data_dir / "equity_minute_bars" / f"{symbol}.parquet"

    if not options_dir.exists():
        raise FileNotFoundError(f"Options directory not found: {options_dir}")
    if not equity_file.exists():
        raise FileNotFoundError(f"Equity file not found: {equity_file}")

    # Load equity minute bars for spot prices
    print("Loading equity minute bars...")
    equity_df = pl.read_parquet(equity_file)

    # Convert dates to filter
    start_dt = datetime.strptime(start_date, '%Y-%m-%d')
    end_dt = datetime.strptime(end_date, '%Y-%m-%d')

    # Get all option parquet files
    option_files = sorted(options_dir.glob("*.parquet"))
    print(f"Found {len(option_files)} option files")

    daily_results = []

    # Process each trading day
    current_date = start_dt
    while current_date <= end_dt:
        date_str = current_date.strftime('%Y-%m-%d')
        print(f"\nProcessing {date_str}...")

        # Filter equity bars for this day
        day_equity = equity_df.filter(
            pl.col('timestamp').cast(pl.Date) == current_date.date()
        )

        if day_equity.height == 0:
            print(f"  No equity data for {date_str}")
            current_date += timedelta(days=1)
            continue

        # Get average spot for the day
        avg_spot = day_equity['close'].mean()

        # Find ATM strike (closest to avg spot, rounded to nearest $1 or $5)
        if avg_spot < 50:
            atm_strike = round(avg_spot)
        else:
            atm_strike = round(avg_spot / 5) * 5

        print(f"  Avg spot: ${avg_spot:.2f}, ATM strike: ${atm_strike}")

        # Find option files for this date and ATM strike
        # Files are named like: PHR_C_20_2026-01-16.parquet
        call_file = options_dir / f"{symbol}_C_{atm_strike}_{current_date.strftime('%Y-%m-%d')}.parquet"
        put_file = options_dir / f"{symbol}_P_{atm_strike}_{current_date.strftime('%Y-%m-%d')}.parquet"

        # Try to find files with any expiration
        call_files = list(options_dir.glob(f"{symbol}_C_{atm_strike}_*.parquet"))
        put_files = list(options_dir.glob(f"{symbol}_P_{atm_strike}_*.parquet"))

        if not call_files or not put_files:
            print(f"  No ATM options found for strike ${atm_strike}")
            current_date += timedelta(days=1)
            continue

        # Use first expiration available
        call_df = pl.read_parquet(call_files[0])
        put_df = pl.read_parquet(put_files[0])

        # Filter for this day
        call_df = call_df.filter(pl.col('timestamp').cast(pl.Date) == current_date.date())
        put_df = put_df.filter(pl.col('timestamp').cast(pl.Date) == current_date.date())

        if call_df.height == 0 or put_df.height == 0:
            print(f"  No option data for {date_str}")
            current_date += timedelta(days=1)
            continue

        # Get expiration from first row
        expiration_str = call_df['expiration'][0]
        expiration = datetime.strptime(expiration_str, '%Y-%m-%d')

        print(f"  Expiration: {expiration_str}, found {call_df.height} call bars, {put_df.height} put bars")

        # Compute IV for each minute bar
        ivs = []

        for row in call_df.iter_rows(named=True):
            timestamp = row['timestamp']

            # Get spot at this timestamp
            spot_row = day_equity.filter(pl.col('timestamp') == timestamp)
            if spot_row.height == 0:
                continue

            spot = spot_row['close'][0]
            dte = (expiration - timestamp).days
            T = dte / 365.0

            if T <= 0:
                continue

            # Compute IV from call mid price
            mid = (row['close'] + row['open']) / 2.0
            if mid <= 0:
                continue

            iv = implied_volatility(mid, spot, atm_strike, T, 0.0, 'call')
            if iv is not None:
                ivs.append(iv)

        if len(ivs) == 0:
            print(f"  No valid IVs computed")
            current_date += timedelta(days=1)
            continue

        mean_iv = np.mean(ivs)
        median_iv = np.median(ivs)
        std_iv = np.std(ivs)
        min_iv = np.min(ivs)
        max_iv = np.max(ivs)

        print(f"  Mean IV: {mean_iv*100:.1f}% (min: {min_iv*100:.1f}%, max: {max_iv*100:.1f}%, std: {std_iv*100:.1f}%)")

        daily_results.append({
            'date': current_date.date(),
            'spot': avg_spot,
            'atm_strike': atm_strike,
            'mean_iv': mean_iv,
            'median_iv': median_iv,
            'std_iv': std_iv,
            'min_iv': min_iv,
            'max_iv': max_iv,
            'num_samples': len(ivs),
        })

        current_date += timedelta(days=1)

    if not daily_results:
        raise ValueError("No data computed for any day")

    return pl.DataFrame(daily_results)

def main():
    parser = argparse.ArgumentParser(description='Compute intraday mean ATM IV')
    parser.add_argument('--symbol', required=True, help='Stock symbol')
    parser.add_argument('--start', required=True, help='Start date (YYYY-MM-DD)')
    parser.add_argument('--end', required=True, help='End date (YYYY-MM-DD)')
    parser.add_argument('--data-dir', required=True, help='Data directory')
    parser.add_argument('--output', required=True, help='Output parquet file')

    args = parser.parse_args()

    data_dir = Path(args.data_dir)

    # Compute daily mean IVs
    df = compute_daily_mean_iv(
        args.symbol,
        args.start,
        args.end,
        data_dir,
    )

    # Save to parquet
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(output_path)

    print(f"\nSaved {df.height} days to {output_path}")
    print(df)

if __name__ == '__main__':
    main()
