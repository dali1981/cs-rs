#!/usr/bin/env python3
"""Check LNT option notional on 2025-11-07."""

import os
import polars as pl
from datetime import datetime, timezone
from pathlib import Path

SYMBOL = "LNT"
DATE = "2025-11-07"
DATA_DIR = Path(os.getenv("FINQ_DATA_DIR", str(Path.home() / "polygon" / "data")))

# Try to load minute data
minute_file = DATA_DIR / "flatfiles" / "options" / "minute_aggs" / "2025" / DATE / f"{SYMBOL}.parquet"

if minute_file.exists():
    print(f"Loading: {minute_file}")
    df = pl.read_parquet(minute_file)

    print(f"\nColumns: {df.columns}")
    print(f"Rows: {len(df)}")

    # Check if volume column exists
    if "volume" in df.columns:
        total_volume = df["volume"].sum()
        print(f"\nTotal volume: {total_volume:,}")

        # Estimate notional (need spot price)
        # LNT spot around $67-70 on 2025-11-07
        estimated_spot = 67.5
        notional = total_volume * 100 * estimated_spot
        print(f"Estimated notional (@ ${estimated_spot}): ${notional:,.2f}")

        if notional >= 100000:
            print(f"\n✓ PASSES min_notional filter ($100,000)")
        else:
            print(f"\n✗ FAILS min_notional filter ($100,000)")

        # Show volume distribution
        print(f"\nVolume stats:")
        print(df["volume"].describe())

        # Show non-zero volumes
        non_zero = df.filter(pl.col("volume") > 0)
        print(f"\nRows with volume > 0: {len(non_zero)}")

        if len(non_zero) > 0:
            print("\nSample rows with volume:")
            print(non_zero.select(["strike", "expiration", "option_type", "volume", "close"]).head(10))
    else:
        print("\n✗ NO VOLUME COLUMN in data")
else:
    print(f"File not found: {minute_file}")

    # Try EOD
    eod_file = DATA_DIR / "options" / "eod" / SYMBOL / "2025.parquet"
    if eod_file.exists():
        print(f"\nTrying EOD: {eod_file}")
        df = pl.read_parquet(eod_file)
        df = df.filter(pl.col("date") == DATE)
        print(f"Rows for {DATE}: {len(df)}")
        print(f"Columns: {df.columns}")
