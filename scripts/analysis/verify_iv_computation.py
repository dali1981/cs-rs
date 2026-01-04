#!/usr/bin/env python3
"""Verify what happens during IV computation with stale data."""

import polars as pl
from datetime import datetime, timezone
from pathlib import Path
import numpy as np
from scipy.stats import norm
from scipy.optimize import brentq

def black_scholes_price(S, K, T, r, sigma, is_call):
    """Black-Scholes option price."""
    if T <= 0:
        return max(S - K, 0) if is_call else max(K - S, 0)

    d1 = (np.log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * np.sqrt(T))
    d2 = d1 - sigma * np.sqrt(T)

    if is_call:
        return S * norm.cdf(d1) - K * np.exp(-r * T) * norm.cdf(d2)
    else:
        return K * np.exp(-r * T) * norm.cdf(-d2) - S * norm.cdf(-d1)

def implied_volatility(market_price, S, K, T, r, is_call):
    """Compute implied volatility using Brent's method."""
    if T <= 0:
        return None

    if market_price <= 0:
        return None

    # Intrinsic value
    intrinsic = max(S - K, 0) if is_call else max(K - S, 0)
    if market_price < intrinsic:
        return None

    def objective(sigma):
        return black_scholes_price(S, K, T, r, sigma, is_call) - market_price

    try:
        iv = brentq(objective, 0.001, 10.0, xtol=1e-6, maxiter=100)
        return iv
    except:
        return None

def calculate_ttm(option_timestamp, expiration_date, market_close_hour=16):
    """Calculate time to maturity in years."""
    # Simplified: assume market closes at 16:00 ET on expiration date
    expiration_datetime = datetime.combine(expiration_date, datetime.min.time()).replace(
        hour=market_close_hour, tzinfo=timezone.utc
    )

    delta = expiration_datetime - option_timestamp
    ttm_years = delta.total_seconds() / (365.25 * 24 * 3600)
    return max(ttm_years, 0.0)

def verify_iv_computation():
    """Verify IV computation with actual data."""

    # Entry time: 15:55 ET on Nov 3 = 20:55 UTC
    entry_time = datetime(2025, 11, 3, 20, 55, 0, tzinfo=timezone.utc)

    # Load option data
    options_file = Path("/Users/mohamedali/polygon/data/flatfiles/options/minute_aggs/2025/2025-11-03/CRBG.parquet")
    options_df = pl.read_parquet(options_file)

    # Load equity data
    equity_file = Path("/Users/mohamedali/polygon/data/flatfiles/stocks/minute_aggs/2025/2025-11-03/CRBG.parquet")
    equity_df = pl.read_parquet(equity_file)

    # Simulate repository logic - get latest bar per contract
    contracts = (
        options_df
        .filter(pl.col("timestamp") <= entry_time)
        .sort(["strike", "expiration", "option_type", "timestamp"], descending=[False, False, False, True])
        .group_by(["strike", "expiration", "option_type"])
        .agg([
            pl.col("close").first().alias("close"),
            pl.col("timestamp").first().alias("timestamp"),
            pl.col("ticker").first().alias("ticker"),
        ])
        .sort("strike")
    )

    # Spot at entry time
    spot_at_entry = equity_df.filter(pl.col("timestamp") == entry_time).select("close").item()

    print("=" * 80)
    print("IV COMPUTATION VERIFICATION")
    print("=" * 80)
    print(f"Entry time: {entry_time}")
    print(f"Spot at entry: ${spot_at_entry:.2f}")
    print()

    print(f"Contracts available: {contracts.height}")
    print()
    print("=" * 80)
    print("PER-OPTION SPOT (Minute-Aligned) vs ENTRY SPOT")
    print("=" * 80)
    print()

    results = []

    for row in contracts.iter_rows(named=True):
        strike = row["strike"]
        expiration = row["expiration"]
        option_type = row["option_type"]
        option_price = row["close"]
        option_timestamp = row["timestamp"]
        ticker = row["ticker"]

        # Get spot at option's timestamp (minute-aligned approach)
        spot_at_option_time_df = equity_df.filter(pl.col("timestamp") == option_timestamp)

        if spot_at_option_time_df.height > 0:
            spot_at_option_time = spot_at_option_time_df.select("close").item()
            spot_available = True
        else:
            spot_at_option_time = None
            spot_available = False

        # Calculate TTM
        ttm_years = calculate_ttm(option_timestamp, expiration)

        # Compute IV with per-option spot (if available)
        if spot_available and ttm_years > 0:
            iv_minute_aligned = implied_volatility(
                option_price,
                spot_at_option_time,
                strike,
                ttm_years,
                0.0,  # r = 0
                option_type == "call"
            )
        else:
            iv_minute_aligned = None

        # Compute IV with entry spot (single spot approach)
        ttm_years_entry = calculate_ttm(entry_time, expiration)
        if ttm_years_entry > 0:
            iv_entry_spot = implied_volatility(
                option_price,
                spot_at_entry,
                strike,
                ttm_years_entry,
                0.0,
                option_type == "call"
            )
        else:
            iv_entry_spot = None

        # Staleness
        staleness_minutes = (entry_time - option_timestamp).total_seconds() / 60

        results.append({
            'ticker': ticker,
            'strike': strike,
            'type': option_type,
            'price': option_price,
            'timestamp': option_timestamp,
            'staleness_min': staleness_minutes,
            'spot_at_option_time': spot_at_option_time if spot_available else None,
            'spot_at_entry': spot_at_entry,
            'iv_minute_aligned': iv_minute_aligned,
            'iv_entry_spot': iv_entry_spot,
            'spot_available': spot_available,
        })

    # Print results
    print(f"{'Ticker':<30} {'Strike':<7} {'Type':<5} {'Price':<8} {'Stale(min)':<11} {'Spot@Option':<12} {'Spot@Entry':<12} {'IV(min-al)':<12} {'IV(entry)':<12} {'Status'}")
    print("-" * 160)

    filtered_minute_aligned = 0
    filtered_entry_spot = 0

    for r in results:
        spot_opt_str = f"${r['spot_at_option_time']:.2f}" if r['spot_at_option_time'] else "NO SPOT"
        iv_min_str = f"{r['iv_minute_aligned']*100:.1f}%" if r['iv_minute_aligned'] is not None else "FAIL"
        iv_entry_str = f"{r['iv_entry_spot']*100:.1f}%" if r['iv_entry_spot'] is not None else "FAIL"

        # Check validation (IV < 0.01 or > 5.0)
        status_parts = []

        if r['iv_minute_aligned'] is None or not r['spot_available']:
            status_parts.append("MIN-AL:SKIP")
            filtered_minute_aligned += 1
        elif r['iv_minute_aligned'] < 0.01 or r['iv_minute_aligned'] > 5.0:
            status_parts.append("MIN-AL:FILTERED")
            filtered_minute_aligned += 1
        else:
            status_parts.append("MIN-AL:OK")

        if r['iv_entry_spot'] is None:
            status_parts.append("ENTRY:FAIL")
            filtered_entry_spot += 1
        elif r['iv_entry_spot'] < 0.01 or r['iv_entry_spot'] > 5.0:
            status_parts.append("ENTRY:FILTERED")
            filtered_entry_spot += 1
        else:
            status_parts.append("ENTRY:OK")

        status = " | ".join(status_parts)

        print(f"{r['ticker']:<30} {r['strike']:<7.1f} {r['type']:<5} ${r['price']:<7.2f} {r['staleness_min']:<11.1f} {spot_opt_str:<12} ${r['spot_at_entry']:<11.2f} {iv_min_str:<12} {iv_entry_str:<12} {status}")

    print()
    print("=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print(f"Total contracts: {len(results)}")
    print()
    print(f"Minute-Aligned IV:")
    print(f"  Filtered/Failed: {filtered_minute_aligned}/{len(results)} ({100*filtered_minute_aligned/len(results):.1f}%)")
    print(f"  Available for IV surface: {len(results) - filtered_minute_aligned}")
    print()
    print(f"Entry-Spot IV:")
    print(f"  Filtered/Failed: {filtered_entry_spot}/{len(results)} ({100*filtered_entry_spot/len(results):.1f}%)")
    print(f"  Available for IV surface: {len(results) - filtered_entry_spot}")
    print()
    print("=" * 80)

if __name__ == "__main__":
    verify_iv_computation()
