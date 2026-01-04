#!/usr/bin/env python3
"""
Debug script to examine the IV surface for CRBG strike 32 failure.

Simulates what the Rust code sees when trying to price CRBG strike 32 call on Dec 19.
"""

import polars as pl
from datetime import datetime, date
from pathlib import Path
import os


def load_crbg_data():
    """Load CRBG option chain data for Nov 3, 2025."""
    data_dir = Path(os.getenv("FINQ_DATA_DIR", Path.home() / "polygon" / "data"))

    # Specific path for Nov 3, 2025
    crbg_file = data_dir / "flatfiles" / "options" / "minute_aggs" / "2025" / "2025-11-03" / "CRBG.parquet"

    if not crbg_file.exists():
        print(f"CRBG file not found: {crbg_file}")
        return None

    print(f"Loading: {crbg_file}")
    df = pl.read_parquet(crbg_file)
    print(f"Loaded {len(df)} rows")
    return df


def compute_iv(price: float, spot: float, strike: float, ttm: float, is_call: bool) -> float | None:
    """Compute implied volatility using Newton-Raphson."""
    from scipy.stats import norm
    import numpy as np

    if price <= 0 or spot <= 0 or strike <= 0 or ttm <= 0:
        return None

    # Intrinsic value check
    intrinsic = max(spot - strike, 0) if is_call else max(strike - spot, 0)
    if price < intrinsic:
        return None  # Arbitrage

    sigma = 0.3  # Initial guess
    for _ in range(100):
        d1 = (np.log(spot / strike) + 0.5 * sigma**2 * ttm) / (sigma * np.sqrt(ttm))
        d2 = d1 - sigma * np.sqrt(ttm)

        if is_call:
            calc_price = spot * norm.cdf(d1) - strike * norm.cdf(d2)
        else:
            calc_price = strike * norm.cdf(-d2) - spot * norm.cdf(-d1)

        diff = calc_price - price
        if abs(diff) < 1e-6:
            return sigma

        # Vega
        vega = spot * norm.pdf(d1) * np.sqrt(ttm)
        if vega < 1e-10:
            return None

        sigma = sigma - diff / vega
        if sigma <= 0:
            return None

    return None


def analyze_nov3_data(df):
    """Analyze option chain data around Nov 3, 2025."""

    target_date = date(2025, 11, 3)
    target_exp = date(2025, 12, 19)

    print("\n" + "=" * 80)
    print(f"ANALYZING: {target_date} pricing for {target_exp} expiration")
    print("=" * 80)

    # Filter to Nov 3 data
    df_nov3 = df.filter(
        pl.col("timestamp").dt.date() == target_date
    )

    print(f"\nTotal bars on {target_date}: {len(df_nov3)}")

    if len(df_nov3) == 0:
        print("No data for this date!")
        return

    # Show unique timestamps
    timestamps = df_nov3.select("timestamp").unique().sort("timestamp")
    print(f"\nUnique timestamps: {len(timestamps)}")
    print(timestamps.head(10))

    # Filter to Dec 19 expiration
    df_dec19 = df_nov3.filter(
        pl.col("expiration") == target_exp
    )

    print(f"\nBars for {target_exp} expiration: {len(df_dec19)}")

    if len(df_dec19) == 0:
        print("No data for this expiration!")
        return

    # Show calls
    df_calls = df_dec19.filter(pl.col("option_type") == "call")
    print(f"\nCalls: {len(df_calls)}")

    # Get unique strikes
    strikes = df_calls.select("strike").unique().sort("strike")
    print(f"\nAvailable call strikes for {target_exp}:")
    print(strikes)

    # Check if strike 32 exists
    strike_32_calls = df_calls.filter(pl.col("strike") == 32.0)
    print(f"\nStrike 32.0 call bars: {len(strike_32_calls)}")

    if len(strike_32_calls) > 0:
        print("Strike 32 HAS DATA!")
        print(strike_32_calls.select(["timestamp", "strike", "close", "volume"]))
    else:
        print("Strike 32 MISSING - will need interpolation")

    # Show strikes 34 and 35 (the available ones from the error message)
    print("\n" + "-" * 80)
    print("Strike 34.0 calls:")
    strike_34 = df_calls.filter(pl.col("strike") == 34.0)
    if len(strike_34) > 0:
        print(strike_34.select(["timestamp", "strike", "close", "volume"]).head(10))
    else:
        print("NO DATA")

    print("\nStrike 35.0 calls:")
    strike_35 = df_calls.filter(pl.col("strike") == 35.0)
    if len(strike_35) > 0:
        print(strike_35.select(["timestamp", "strike", "close", "volume"]).head(10))
    else:
        print("NO DATA")

    # Check puts at strike 32
    print("\n" + "-" * 80)
    print("Strike 32.0 PUTS (for put-call parity):")
    df_puts = df_dec19.filter(pl.col("option_type") == "put")
    strike_32_puts = df_puts.filter(pl.col("strike") == 32.0)
    print(f"Strike 32.0 put bars: {len(strike_32_puts)}")

    if len(strike_32_puts) > 0:
        print(strike_32_puts.select(["timestamp", "strike", "close", "volume"]).head(10))

    # Check Nov 21 expiration (mentioned in error message)
    print("\n" + "=" * 80)
    print("CHECKING: Nov 21 expiration (seen in error message)")
    print("=" * 80)

    nov21_exp = date(2025, 11, 21)
    df_nov21 = df_nov3.filter(pl.col("expiration") == nov21_exp)
    print(f"Bars for {nov21_exp} expiration: {len(df_nov21)}")

    if len(df_nov21) > 0:
        strikes_nov21 = df_nov21.select(["strike", "option_type"]).unique().sort(["option_type", "strike"])
        print("\nAvailable strikes for Nov 21:")
        print(strikes_nov21)

        # Check strike 31 put (mentioned in error message)
        strike_31_puts = df_nov21.filter(
            (pl.col("strike") == 31.0) & (pl.col("option_type") == "put")
        )
        if len(strike_31_puts) > 0:
            print("\nStrike 31.0 put (Nov 21):")
            print(strike_31_puts.select(["timestamp", "strike", "close", "volume"]).head(5))

    # Summary
    print("\n" + "=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print(f"Target: Price CRBG call strike 32, exp Dec 19, on Nov 3")
    print(f"Available Dec 19 call strikes: {strikes.to_series().to_list()}")
    print(f"Strike 32 call exists: {'YES' if len(strike_32_calls) > 0 else 'NO'}")
    print(f"Strike 32 put exists: {'YES' if len(strike_32_puts) > 0 else 'NO'}")
    print()

    if len(strike_32_calls) == 0:
        print("INTERPOLATION NEEDED:")
        print("  1. No market data for strike 32 call")
        print("  2. Put-call parity needs strike 32 put (also missing)")
        print("  3. IV interpolation must use strikes 34, 35")
        print("  4. BUT: Both 34, 35 are ABOVE target strike 32")
        print("  5. Flat extrapolation should use strike 34's IV")
        print()
        print("Expected behavior:")
        print("  - Sticky moneyness: target moneyness = 32/32 = 1.0")
        print("  - Available moneyness: 34/32 = 1.0625, 35/32 = 1.09375")
        print("  - Both > 1.0, so should extrapolate using 1.0625's IV")


def check_data_at_entry_time(df):
    """Check what data exists at entry/exit times."""
    from datetime import datetime, timezone

    target_exp = date(2025, 12, 19)

    # Actual strategy timing: entry 15:55 ET, exit 9:45 ET
    # For Nov 3-4: EST = UTC-5
    times_to_check = [
        ("Entry 15:55 ET (Nov 3)",   datetime(2025, 11, 3, 20, 55, 0, tzinfo=timezone.utc)),
        ("Exit 9:45 ET (Nov 4)",     datetime(2025, 11, 4, 14, 45, 0, tzinfo=timezone.utc)),
    ]

    print("\n" + "=" * 80)
    print("TIMING ANALYSIS - Data Availability at Entry/Exit Times")
    print("=" * 80)

    for label, check_time in times_to_check:
        check_date = check_time.date()

        print(f"\n{'='*60}")
        print(f"{label}: {check_time}")
        print("=" * 60)

        # Load data for that date
        data_dir = Path(os.getenv("FINQ_DATA_DIR", Path.home() / "polygon" / "data"))
        data_file = data_dir / "flatfiles" / "options" / "minute_aggs" / "2025" / check_date.strftime("%Y-%m-%d") / "CRBG.parquet"

        if not data_file.exists():
            print(f"  NO DATA FILE for {check_date}")
            continue

        df_day = pl.read_parquet(data_file)
        print(f"  Total bars on {check_date}: {len(df_day)}")

        # Filter to trades at or before check time
        df_at_time = df_day.filter(pl.col("timestamp") <= pl.lit(check_time))
        print(f"  Bars at or before {check_time.strftime('%H:%M')} UTC: {len(df_at_time)}")

        # Check Dec 19 calls specifically
        df_dec19_calls = df_at_time.filter(
            (pl.col("expiration") == target_exp) & (pl.col("option_type") == "call")
        )
        print(f"  Dec 19 CALLS available: {len(df_dec19_calls)} bars")

        if len(df_dec19_calls) > 0:
            strikes = df_dec19_calls.select("strike").unique().sort("strike")["strike"].to_list()
            print(f"  Dec 19 call strikes: {strikes}")

            # Compute IVs for each strike to verify IV surface would be built
            print("  IV verification:")
            spot = 32.0  # approximate spot
            ttm = (target_exp - check_date).days / 365.0

            for row in df_dec19_calls.iter_rows(named=True):
                strike = row["strike"]
                price = row["close"]
                iv = compute_iv(price, spot, strike, ttm, is_call=True)
                if iv is not None:
                    print(f"    Strike {strike}: price=${price:.2f}, IV={iv*100:.1f}%")
                else:
                    print(f"    Strike {strike}: price=${price:.2f}, IV=FAILED")
        else:
            print("  *** NO DEC 19 CALL DATA ***")

            # Show when Dec 19 call data first appears
            df_dec19_all = df_day.filter(
                (pl.col("expiration") == target_exp) & (pl.col("option_type") == "call")
            )
            if len(df_dec19_all) > 0:
                first_trade = df_dec19_all.select(pl.col("timestamp").min())[0, 0]
                print(f"  First Dec 19 call trade: {first_trade}")
            else:
                print("  No Dec 19 call data at all on this date")


def main():
    """Main analysis."""
    print("CRBG IV Surface Debug Analysis")
    print("=" * 80)

    df = load_crbg_data()
    if df is None:
        return

    # Show schema
    print("\nSchema:")
    print(df.schema)

    analyze_nov3_data(df)
    check_data_at_entry_time(df)


if __name__ == '__main__':
    main()
