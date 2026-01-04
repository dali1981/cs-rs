#!/usr/bin/env python3
"""
Create a custom earnings parquet file for backtesting.

Usage:
    python create_custom_earnings.py ANGO "01-08-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO" --output earnings_2025.parquet

Input format: "DD-MM-YYYY BMO/AMC, DD-MM-YYYY BMO/AMC, ..."
"""

import argparse
import polars as pl
from datetime import datetime
from pathlib import Path


def parse_earnings_string(earnings_str: str) -> list[tuple[datetime, str]]:
    """Parse earnings string into list of (date, time) tuples."""
    earnings = []

    for item in earnings_str.split(","):
        item = item.strip()
        parts = item.split()
        if len(parts) != 2:
            raise ValueError(f"Invalid format: '{item}'. Expected 'DD-MM-YYYY BMO/AMC'")

        date_str, time_str = parts
        time_str = time_str.upper()

        if time_str not in ("BMO", "AMC"):
            raise ValueError(f"Invalid earnings time: '{time_str}'. Must be BMO or AMC")

        # Try multiple date formats
        date = None
        for fmt in ("%d-%m-%Y", "%d/%m/%Y", "%Y-%m-%d"):
            try:
                date = datetime.strptime(date_str, fmt)
                break
            except ValueError:
                continue

        if date is None:
            raise ValueError(f"Invalid date format: '{date_str}'. Use DD-MM-YYYY or DD/MM/YYYY")

        earnings.append((date, time_str))

    return sorted(earnings, key=lambda x: x[0])


def create_earnings_parquet(
    symbol: str,
    earnings: list[tuple[datetime, str]],
    output_path: Path,
    company_name: str | None = None,
    market_cap: int | None = None,
):
    """Create earnings parquet file with the schema expected by the backtest system."""

    # Convert to Polars date (days since Unix epoch)
    epoch = datetime(1970, 1, 1)

    data = {
        "symbol": [symbol] * len(earnings),
        "earnings_date": [int((e[0] - epoch).days) for e in earnings],
        "earnings_time": [e[1] for e in earnings],
    }

    if company_name:
        data["company_name"] = [company_name] * len(earnings)

    if market_cap:
        data["market_cap"] = [market_cap] * len(earnings)

    # Create DataFrame with proper types
    df = pl.DataFrame(data)

    # Cast to proper types
    df = df.with_columns([
        pl.col("earnings_date").cast(pl.Date),
        pl.col("symbol").cast(pl.Utf8),
        pl.col("earnings_time").cast(pl.Utf8),
    ])

    # Save to parquet
    output_path.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(output_path)

    return df


def main():
    parser = argparse.ArgumentParser(
        description="Create custom earnings parquet file for backtesting",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
    # Single symbol with multiple earnings dates
    python create_custom_earnings.py ANGO "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO"

    # With custom output path
    python create_custom_earnings.py ANGO "08-01-2025 BMO, 02-04-2025 BMO" --output ./custom_earnings/earnings_2025.parquet

    # With company info
    python create_custom_earnings.py ANGO "08-01-2025 BMO" --company-name "AngioDynamics Inc" --market-cap 250000000
""")

    parser.add_argument("symbol", help="Stock symbol (e.g., ANGO)")
    parser.add_argument("earnings", help='Earnings dates: "DD-MM-YYYY BMO/AMC, DD-MM-YYYY BMO/AMC, ..."')
    parser.add_argument("--output", "-o", type=Path, help="Output parquet file path")
    parser.add_argument("--company-name", help="Company name (optional)")
    parser.add_argument("--market-cap", type=int, help="Market cap in dollars (optional)")

    args = parser.parse_args()

    # Parse earnings string
    earnings = parse_earnings_string(args.earnings)

    # Default output path
    if args.output is None:
        year = earnings[0][0].year if earnings else 2025
        args.output = Path(f"./earnings_{year}.parquet")

    # Create parquet file
    df = create_earnings_parquet(
        symbol=args.symbol.upper(),
        earnings=earnings,
        output_path=args.output,
        company_name=args.company_name,
        market_cap=args.market_cap,
    )

    print(f"Created earnings file: {args.output}")
    print(f"\nEarnings events for {args.symbol.upper()}:")
    print("-" * 50)

    for i, (date, time) in enumerate(earnings, 1):
        print(f"  {i}. {date.strftime('%Y-%m-%d')} {time}")

    print(f"\nSchema:")
    print(df.schema)
    print(f"\nData:")
    print(df)


if __name__ == "__main__":
    main()
