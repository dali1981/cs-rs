#!/usr/bin/env python3
"""Test the actual repository aggregation logic."""

import polars as pl
from datetime import datetime, timezone
from pathlib import Path

def test_repository_logic():
    """Simulate get_option_bars_at_time repository logic."""

    # Entry time: 15:55 ET on Nov 3 = 20:55 UTC
    entry_time = datetime(2025, 11, 3, 20, 55, 0, tzinfo=timezone.utc)
    target_nanos = int(entry_time.timestamp() * 1_000_000_000)

    # Load option data
    options_file = Path("/Users/mohamedali/polygon/data/flatfiles/options/minute_aggs/2025/2025-11-03/CRBG.parquet")
    df = pl.read_parquet(options_file)

    print("=" * 80)
    print("REPOSITORY LOGIC TEST: get_option_bars_at_time")
    print("=" * 80)
    print(f"Target time: {entry_time}")
    print()

    # Mimic repository logic exactly
    filtered = (
        df
        .lazy()
        .filter(pl.col("timestamp") <= entry_time)  # At or before target
        .sort(
            ["strike", "expiration", "option_type", "timestamp"],
            descending=[False, False, False, True]  # timestamp DESC
        )
        .group_by(["strike", "expiration", "option_type"])  # Group by contract
        .agg([
            pl.col("close").first().alias("close"),  # Take first (latest due to sort)
            pl.col("timestamp").first().alias("timestamp"),
            pl.col("ticker").first().alias("ticker"),
        ])
        .collect()
    )

    print(f"Result after repository aggregation:")
    print(f"  Total contracts: {filtered.height}")
    print()

    if filtered.height > 0:
        print("Available contracts:")
        for row in filtered.select(["ticker", "strike", "option_type", "close", "timestamp"]).sort("strike").iter_rows(named=True):
            print(f"  Strike {row['strike']:5.1f} {row['option_type']:4s}: ${row['close']:6.2f} @ {row['timestamp']} ({row['ticker']})")

    print()
    print("=" * 80)
    print()

    # Show what bars contributed to each contract
    print("Source bars (what bars exist ≤ target time):")
    all_bars = df.filter(pl.col("timestamp") <= entry_time).select(
        ["timestamp", "ticker", "strike", "option_type", "close"]
    ).sort(["strike", "timestamp"])

    print(f"  Total bars: {all_bars.height}")
    print()

    for row in all_bars.iter_rows(named=True):
        print(f"  {row['timestamp']} Strike {row['strike']:5.1f} {row['option_type']:4s}: ${row['close']:6.2f}")

    print()
    print("=" * 80)

if __name__ == "__main__":
    test_repository_logic()
