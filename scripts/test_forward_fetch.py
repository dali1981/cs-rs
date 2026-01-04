#!/usr/bin/env python3
"""Test forward-looking option data fetch for CRBG."""

import polars as pl
from datetime import datetime, timezone
from pathlib import Path

def test_forward_fetch():
    """Simulate the forward-looking fetch logic."""

    # CRBG data files (need both Nov 3 and Nov 4)
    data_files = [
        Path("/Users/mohamedali/polygon/data/flatfiles/options/minute_aggs/2025/2025-11-03/CRBG.parquet"),
        Path("/Users/mohamedali/polygon/data/flatfiles/options/minute_aggs/2025/2025-11-04/CRBG.parquet"),
    ]

    for f in data_files:
        if not f.exists():
            print(f"❌ File not found: {f}")
            return

    # Load and concatenate data
    dfs = [pl.read_parquet(f) for f in data_files]
    df = pl.concat(dfs)

    # Target exit time: 9:45 AM ET on Nov 4 = 14:45 UTC
    exit_time = datetime(2025, 11, 4, 14, 45, 0, tzinfo=timezone.utc)
    exit_nanos = int(exit_time.timestamp() * 1_000_000_000)

    # Max forward: 30 minutes
    max_forward_minutes = 30
    max_forward_nanos = exit_nanos + (max_forward_minutes * 60 * 1_000_000_000)

    print("=" * 80)
    print("FORWARD-LOOKING FETCH TEST")
    print("=" * 80)
    print()
    print(f"Target exit time:     {exit_time} ({exit_nanos} ns)")
    print(f"Max forward time:     {datetime.fromtimestamp(max_forward_nanos / 1e9, tz=timezone.utc)}")
    print(f"Lookahead window:     {max_forward_minutes} minutes")
    print()

    # Try backward lookup first (existing behavior)
    backward = df.filter(pl.col("timestamp") <= exit_time)

    print(f"Backward lookup (≤ target time):")
    if backward.height > 0:
        latest_time = backward.select(pl.col("timestamp").max()).item()
        print(f"  ✅ Found {backward.height} bars")
        print(f"  Latest timestamp: {latest_time}")
        print()
    else:
        print(f"  ❌ No data found at or before target time")
        print()

    # Try forward lookup
    max_forward_time = datetime.fromtimestamp(max_forward_nanos / 1e9, tz=timezone.utc)
    forward = df.filter(
        (pl.col("timestamp") > exit_time) &
        (pl.col("timestamp") <= max_forward_time)
    ).sort("timestamp")

    print(f"Forward lookup (> target AND ≤ max forward):")
    if forward.height > 0:
        first_time = forward.select(pl.col("timestamp").min()).item()
        delta_seconds = (first_time - exit_time).total_seconds()
        delta_minutes = delta_seconds / 60

        print(f"  ✅ Found {forward.height} bars")
        print(f"  First timestamp:  {first_time}")
        print(f"  Delta from target: +{delta_minutes:.1f} minutes")
        print()

        # Group by contract and get first bar per contract
        contracts = forward.group_by("ticker").agg(pl.col("timestamp").min().alias("first_timestamp"))
        print(f"  Unique contracts: {contracts.height}")
        print()
        print("  Sample contracts with first trade time:")
        for row in contracts.head(10).iter_rows(named=True):
            first_ts = row["first_timestamp"]
            delta_seconds = (first_ts - exit_time).total_seconds()
            delta_minutes = delta_seconds / 60
            print(f"    {row['ticker']:30s} {first_ts} (+{delta_minutes:.1f} min)")
    else:
        print(f"  ❌ No data found in forward window")

    print()
    print("=" * 80)

if __name__ == "__main__":
    test_forward_fetch()
