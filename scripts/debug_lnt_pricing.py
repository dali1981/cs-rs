#!/usr/bin/env python3
"""
Diagnostic script to investigate LNT pricing error on 2025-11-07.

Error: Cannot determine IV for put strike 67.5, expiration 2025-12-19
- No market data
- Put-call parity failed
- Interpolation failed
"""

import os
import polars as pl
from datetime import datetime, timezone
from pathlib import Path

# Configuration
SYMBOL = "LNT"
ENTRY_TIME = datetime(2025, 11, 7, 9, 30, 0, tzinfo=timezone.utc)
PROBLEM_STRIKE = 67.5
PROBLEM_EXPIRATION = "2025-12-19"
DATA_DIR = Path(os.getenv("FINQ_DATA_DIR", str(Path.home() / "polygon" / "data")))

def load_option_chain(symbol: str, timestamp: datetime) -> pl.DataFrame:
    """Load option chain data for a symbol at a specific timestamp."""
    # This is simplified - actual finq loading logic is more complex
    # For now, just try to find the relevant parquet file

    # Try minute bars first
    date_str = timestamp.strftime("%Y-%m-%d")
    minute_file = DATA_DIR / "options" / "minute" / symbol / f"{date_str}.parquet"

    if minute_file.exists():
        print(f"Loading minute data from: {minute_file}")
        df = pl.read_parquet(minute_file)

        # Filter to closest timestamp
        ts_ms = int(timestamp.timestamp() * 1000)
        # Convert to approximate time window
        df = df.filter(pl.col("timestamp") <= ts_ms)

        return df
    else:
        print(f"Minute file not found: {minute_file}")
        # Try daily/EOD data
        eod_file = DATA_DIR / "options" / "eod" / symbol / f"{timestamp.year}.parquet"
        if eod_file.exists():
            print(f"Loading EOD data from: {eod_file}")
            df = pl.read_parquet(eod_file)
            # Filter to date
            df = df.filter(pl.col("date") == date_str)
            return df
        else:
            print(f"EOD file not found: {eod_file}")
            return pl.DataFrame()

def analyze_option_chain(df: pl.DataFrame):
    """Analyze the option chain to understand what data is available."""

    if df.is_empty():
        print("ERROR: No option chain data found!")
        return

    print(f"\n{'='*80}")
    print(f"OPTION CHAIN ANALYSIS")
    print(f"{'='*80}")

    print(f"\nTotal rows: {len(df)}")
    print(f"Columns: {df.columns}")

    # Analyze strikes
    if "strike" in df.columns:
        strikes = df["strike"].unique().sort()
        print(f"\nAvailable strikes ({len(strikes)} total):")
        print(f"  Min: {strikes.min()}")
        print(f"  Max: {strikes.max()}")
        print(f"  All: {strikes.to_list()[:20]}...")  # Show first 20

        # Check if problem strike exists
        if PROBLEM_STRIKE in strikes.to_list():
            print(f"\n✓ Strike {PROBLEM_STRIKE} EXISTS in chain")
        else:
            print(f"\n✗ Strike {PROBLEM_STRIKE} NOT FOUND in chain")
            # Find closest strikes
            strikes_list = sorted(strikes.to_list())
            lower = [s for s in strikes_list if s < PROBLEM_STRIKE]
            upper = [s for s in strikes_list if s > PROBLEM_STRIKE]
            if lower:
                print(f"  Closest lower strike: {lower[-1]}")
            if upper:
                print(f"  Closest upper strike: {upper[0]}")

    # Analyze expirations
    if "expiration" in df.columns:
        expirations = df["expiration"].unique().sort()
        print(f"\nAvailable expirations ({len(expirations)} total):")
        print(f"  {expirations.to_list()}")

        # Check if problem expiration exists
        # Note: expiration is stored as date type in polars
        from datetime import date
        problem_exp_date = date.fromisoformat(PROBLEM_EXPIRATION)
        exp_dates = [date(1970, 1, 1) + pl.timedelta(days=int(e)) for e in expirations.to_list()]

        if problem_exp_date in exp_dates:
            print(f"\n✓ Expiration {PROBLEM_EXPIRATION} EXISTS in chain")
        else:
            print(f"\n✗ Expiration {PROBLEM_EXPIRATION} NOT FOUND in chain")
            print(f"  Available: {[str(d) for d in exp_dates]}")

    # Analyze option types
    if "option_type" in df.columns:
        type_counts = df.group_by("option_type").agg(pl.len().alias("count"))
        print(f"\nOption types:")
        print(type_counts)

    # Check for the specific problem option
    print(f"\n{'='*80}")
    print(f"SEARCHING FOR PROBLEM OPTION")
    print(f"{'='*80}")
    print(f"Strike: {PROBLEM_STRIKE}, Expiration: {PROBLEM_EXPIRATION}, Type: put")

    if "strike" in df.columns and "expiration" in df.columns and "option_type" in df.columns:
        # Convert expiration for comparison
        problem_exp_days = (datetime.fromisoformat(PROBLEM_EXPIRATION).date() - datetime(1970, 1, 1).date()).days

        problem_option = df.filter(
            (pl.col("strike") == PROBLEM_STRIKE) &
            (pl.col("expiration") == problem_exp_days) &
            (pl.col("option_type") == "put")
        )

        if not problem_option.is_empty():
            print(f"\n✓ FOUND problem option in chain:")
            print(problem_option)
        else:
            print(f"\n✗ Problem option NOT FOUND")

            # Check call at same strike
            call_option = df.filter(
                (pl.col("strike") == PROBLEM_STRIKE) &
                (pl.col("expiration") == problem_exp_days) &
                (pl.col("option_type") == "call")
            )

            if not call_option.is_empty():
                print(f"\n✓ FOUND call at same strike/expiration (put-call parity should work):")
                print(call_option)
            else:
                print(f"\n✗ No call at same strike/expiration (put-call parity cannot work)")

    # Analyze puts for IV surface
    print(f"\n{'='*80}")
    print(f"PUT OPTIONS ANALYSIS (for IV surface)")
    print(f"{'='*80}")

    if "option_type" in df.columns:
        puts = df.filter(pl.col("option_type") == "put")
        print(f"\nTotal put options: {len(puts)}")

        if not puts.is_empty() and "strike" in puts.columns and "close" in puts.columns:
            # Filter to valid data (close > 0)
            valid_puts = puts.filter(pl.col("close") > 0)
            print(f"Valid put options (close > 0): {len(valid_puts)}")

            if not valid_puts.is_empty():
                put_strikes = valid_puts["strike"].unique().sort()
                print(f"\nPut strikes available for IV surface:")
                print(f"  Count: {len(put_strikes)}")
                print(f"  Range: {put_strikes.min()} - {put_strikes.max()}")
                print(f"  All: {put_strikes.to_list()}")

                # Check if we can interpolate
                put_strikes_list = sorted(put_strikes.to_list())
                lower_strikes = [s for s in put_strikes_list if s < PROBLEM_STRIKE]
                upper_strikes = [s for s in put_strikes_list if s > PROBLEM_STRIKE]

                if lower_strikes and upper_strikes:
                    print(f"\n✓ Can potentially interpolate between {lower_strikes[-1]} and {upper_strikes[0]}")
                elif lower_strikes:
                    print(f"\n⚠ Can only extrapolate from below (max strike: {lower_strikes[-1]})")
                elif upper_strikes:
                    print(f"\n⚠ Can only extrapolate from above (min strike: {upper_strikes[0]})")
                else:
                    print(f"\n✗ No bracketing strikes available for interpolation")

def main():
    print(f"Investigating LNT pricing error on {ENTRY_TIME}")
    print(f"Data directory: {DATA_DIR}")

    # Load option chain
    df = load_option_chain(SYMBOL, ENTRY_TIME)

    # Analyze
    analyze_option_chain(df)

if __name__ == "__main__":
    main()
