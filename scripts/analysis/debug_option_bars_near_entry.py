#!/usr/bin/env python3
"""Check what option bars exist around entry time."""

import polars as pl
from datetime import datetime, timezone, timedelta
from pathlib import Path

def debug_option_bars():
    """Check option bar availability around entry time."""

    # Entry time: 15:55 ET on Nov 3 = 20:55 UTC
    entry_time = datetime(2025, 11, 3, 20, 55, 0, tzinfo=timezone.utc)

    # Load option data
    options_file = Path("/Users/mohamedali/polygon/data/flatfiles/options/minute_aggs/2025/2025-11-03/CRBG.parquet")
    options_df = pl.read_parquet(options_file)

    print("=" * 80)
    print("OPTION BARS AROUND ENTRY TIME")
    print("=" * 80)
    print(f"Entry time: {entry_time}")
    print()

    # Filter to ±30 minutes around entry
    window_start = entry_time - timedelta(minutes=30)
    window_end = entry_time + timedelta(minutes=30)

    options_window = options_df.filter(
        (pl.col("timestamp") >= window_start) &
        (pl.col("timestamp") <= window_end)
    ).select(["timestamp", "ticker", "strike", "option_type", "close"]).sort("timestamp")

    print(f"Option bars in ±30 min window:")
    print(f"  Total bars: {options_window.height}")
    print()

    if options_window.height > 0:
        # Group by timestamp to see bar count per minute
        bars_per_minute = options_window.group_by("timestamp").agg(
            pl.count().alias("bar_count"),
            pl.col("strike").unique().alias("strikes")
        ).sort("timestamp")

        print("Bars per minute:")
        for row in bars_per_minute.iter_rows(named=True):
            marker = "  *** ENTRY ***" if row["timestamp"] == entry_time else ""
            strikes_str = ", ".join([f"{s:.0f}" for s in sorted(row["strikes"])])
            print(f"  {row['timestamp']}: {row['bar_count']:2d} bars, strikes=[{strikes_str}]{marker}")

    print()
    print("=" * 80)
    print()

    # What happens if we use get_option_bars_at_time (backward lookup)?
    backward = options_df.filter(pl.col("timestamp") <= entry_time)

    if backward.height > 0:
        # Get latest timestamp before/at entry
        latest_time = backward.select(pl.col("timestamp").max()).item()
        latest_bars = backward.filter(pl.col("timestamp") == latest_time)

        print(f"get_option_bars_at_time (backward lookup ≤ {entry_time}):")
        print(f"  Latest timestamp: {latest_time}")
        print(f"  Bars at latest time: {latest_bars.height}")
        print()

        if latest_time == entry_time:
            print("  ✅ Data exists at exact entry time")
        else:
            delta_min = (entry_time - latest_time).total_seconds() / 60
            print(f"  ⚠️  Using stale data from {delta_min:.1f} minutes ago")

        print()
        print("  Available strikes:")
        for row in latest_bars.select(["ticker", "strike", "option_type", "close"]).sort("strike").iter_rows(named=True):
            print(f"    Strike {row['strike']:5.1f} {row['option_type']:4s}: ${row['close']:.2f} ({row['ticker']})")

    print()
    print("=" * 80)

if __name__ == "__main__":
    debug_option_bars()
