#!/usr/bin/env python3
"""Debug spot price availability for CRBG options at entry time."""

import polars as pl
from datetime import datetime, timezone
from pathlib import Path

def debug_entry_time():
    """Check spot price availability at entry time 15:55."""

    # Entry time: 15:55 ET on Nov 3 = 20:55 UTC
    entry_time = datetime(2025, 11, 3, 20, 55, 0, tzinfo=timezone.utc)

    # Load option data
    options_file = Path("/Users/mohamedali/polygon/data/flatfiles/options/minute_aggs/2025/2025-11-03/CRBG.parquet")
    options_df = pl.read_parquet(options_file)

    # Load equity data
    equity_file = Path("/Users/mohamedali/polygon/data/flatfiles/stocks/minute_aggs/2025/2025-11-03/CRBG.parquet")
    if not equity_file.exists():
        print(f"❌ Equity file not found: {equity_file}")
        return

    equity_df = pl.read_parquet(equity_file)

    print("=" * 80)
    print("ENTRY TIME SPOT PRICE DEBUG")
    print("=" * 80)
    print(f"Entry time: {entry_time}")
    print()

    # Filter options at entry time
    options_at_entry = options_df.filter(pl.col("timestamp") == entry_time)

    print(f"Options bars at entry time ({entry_time}):")
    print(f"  Count: {options_at_entry.height}")

    if options_at_entry.height > 0:
        print()
        print("  Contracts:")
        for row in options_at_entry.select(["ticker", "strike", "option_type", "close"]).iter_rows(named=True):
            print(f"    {row['ticker']:30s} strike={row['strike']:6.1f} {row['option_type']:4s} close=${row['close']:.2f}")
    print()

    # Check equity bars around entry time
    print("Equity (spot) bars around entry time:")
    equity_window = equity_df.filter(
        (pl.col("timestamp") >= datetime(2025, 11, 3, 20, 45, 0, tzinfo=timezone.utc)) &
        (pl.col("timestamp") <= datetime(2025, 11, 3, 21, 0, 0, tzinfo=timezone.utc))
    ).sort("timestamp")

    print(f"  Total bars in ±5 min window: {equity_window.height}")

    if equity_window.height > 0:
        print()
        for row in equity_window.select(["timestamp", "close", "volume"]).iter_rows(named=True):
            marker = "  *** ENTRY TIME ***" if row["timestamp"] == entry_time else ""
            print(f"    {row['timestamp']} close=${row['close']:.2f} vol={row['volume']:>6}{marker}")
    print()

    # Check if spot price exists at exact entry time
    spot_at_entry = equity_df.filter(pl.col("timestamp") == entry_time)

    if spot_at_entry.height > 0:
        spot_price = spot_at_entry.select(pl.col("close")).item()
        print(f"✅ Spot price at exact entry time: ${spot_price:.2f}")
    else:
        print(f"❌ NO spot price at exact entry time ({entry_time})")

        # Find nearest spot price
        before = equity_df.filter(pl.col("timestamp") < entry_time).select(["timestamp", "close"]).sort("timestamp", descending=True).head(1)
        after = equity_df.filter(pl.col("timestamp") > entry_time).select(["timestamp", "close"]).sort("timestamp").head(1)

        if before.height > 0:
            before_time = before.select("timestamp").item()
            before_price = before.select("close").item()
            delta_min = (entry_time - before_time).total_seconds() / 60
            print(f"   Nearest before: {before_time} (${before_price:.2f}, {delta_min:.1f} min earlier)")

        if after.height > 0:
            after_time = after.select("timestamp").item()
            after_price = after.select("close").item()
            delta_min = (after_time - entry_time).total_seconds() / 60
            print(f"   Nearest after:  {after_time} (${after_price:.2f}, {delta_min:.1f} min later)")

    print()
    print("=" * 80)
    print()
    print("IMPACT ON IV SURFACE BUILDER:")
    print()
    print("For minute-aligned IV computation, each option bar requires:")
    print("  1. Option price at timestamp T")
    print("  2. Spot price at EXACT timestamp T")
    print()
    print("If spot price missing at T → option is SKIPPED from IV surface")
    print()

    if spot_at_entry.height == 0:
        print("⚠️  PROBLEM IDENTIFIED:")
        print("   CRBG didn't trade at 15:55 ET → no spot price at entry time")
        print("   → All option bars get filtered out")
        print("   → IV surface is empty or sparse")
        print("   → Interpolation fails (no points to interpolate from)")

    print()
    print("=" * 80)

if __name__ == "__main__":
    debug_entry_time()
