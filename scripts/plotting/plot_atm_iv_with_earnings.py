#!/usr/bin/env python3
"""
Plot ATM IV time series with multiple earnings dates as vertical lines.

Usage:
    python plot_atm_iv_with_earnings.py <iv_parquet> --earnings "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO" --output output.png

    # Or load earnings from parquet file
    python plot_atm_iv_with_earnings.py <iv_parquet> --earnings-file earnings_2025.parquet --output output.png
"""

import argparse
import polars as pl
import matplotlib.pyplot as plt
import matplotlib.dates as mdates
from datetime import datetime, timedelta
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


def load_earnings_from_parquet(path: Path, symbol: str | None = None) -> list[tuple[datetime, str]]:
    """Load earnings from parquet file."""
    df = pl.read_parquet(path)

    if symbol:
        df = df.filter(pl.col("symbol") == symbol)

    # Convert date from polars Date
    epoch = datetime(1970, 1, 1)

    earnings = []
    for row in df.iter_rows(named=True):
        if isinstance(row["earnings_date"], int):
            date = epoch + timedelta(days=row["earnings_date"])
        else:
            # Already a date object
            date = datetime.combine(row["earnings_date"], datetime.min.time())
        time_str = row["earnings_time"]
        earnings.append((date, time_str))

    return sorted(earnings, key=lambda x: x[0])


def plot_iv_with_earnings(
    iv_file: Path,
    earnings: list[tuple[datetime, str]],
    output_file: Path | None = None,
    maturities: list[str] = ["7d", "14d", "30d"],
    show_spread: bool = True,
):
    """Plot IV time series with earnings dates as vertical lines."""

    # Read IV data
    df = pl.read_parquet(iv_file)

    # Convert date from days since epoch
    epoch = datetime(1970, 1, 1)
    df = df.with_columns([
        pl.col('date').map_elements(
            lambda d: epoch + timedelta(days=d),
            return_dtype=pl.Datetime
        ).alias('datetime')
    ])

    # Convert to pandas for plotting
    df_pd = df.to_pandas()
    symbol = df["symbol"].unique()[0]

    # Colors for earnings lines (cycling through if more than 4)
    earnings_colors = ['#9b59b6', '#e91e63', '#00bcd4', '#ff9800', '#4caf50', '#f44336']

    # Create figure with subplots
    n_panels = 2 if show_spread else 1
    fig, axes = plt.subplots(n_panels, 1, figsize=(18, 6 * n_panels), sharex=True)
    if n_panels == 1:
        axes = [axes]

    ax1 = axes[0]

    # Color map for maturities
    maturity_colors = {
        "7d": '#e74c3c',
        "14d": '#3498db',
        "21d": '#f39c12',
        "30d": '#2ecc71',
        "60d": '#9b59b6',
        "90d": '#1abc9c',
    }

    # Plot IVs
    for mat in maturities:
        col = f'cm_iv_{mat}'
        if col in df_pd.columns:
            color = maturity_colors.get(mat, 'gray')
            ax1.plot(df_pd['datetime'], df_pd[col] * 100,
                     label=f'{mat} IV', linewidth=2.5, alpha=0.9, color=color)

    # Add earnings lines
    for i, (date, time_str) in enumerate(earnings):
        color = earnings_colors[i % len(earnings_colors)]
        label = f'{date.strftime("%b %d")} ({time_str})'
        ax1.axvline(x=date, color=color, linestyle='--', linewidth=2.5,
                    alpha=0.8, label=f'Earnings: {label}')

        # Add annotation
        ymin, ymax = ax1.get_ylim()
        ax1.annotate(f'Q{(i % 4) + 1}\n{time_str}',
                     xy=(date, ymax * 0.95),
                     xytext=(date, ymax * 0.95),
                     fontsize=10, fontweight='bold',
                     ha='center', va='top',
                     color=color,
                     bbox=dict(boxstyle='round,pad=0.3', facecolor='white', alpha=0.8, edgecolor=color))

    ax1.set_ylabel('Implied Volatility (%)', fontsize=14, fontweight='bold')
    ax1.set_title(f'{symbol} - ATM Implied Volatility with Earnings Events (2025)\nConstant-Maturity IV Term Structure',
                  fontsize=16, fontweight='bold', pad=15)
    ax1.legend(fontsize=11, loc='upper left', framealpha=0.95, ncol=2)
    ax1.grid(True, alpha=0.3, linestyle='--')
    ax1.set_ylim(bottom=0)

    # Panel 2: Earnings detection spread (7d - 30d)
    if show_spread and 'cm_spread_7_30' in df_pd.columns:
        ax2 = axes[1]
        spread = df_pd['cm_spread_7_30'] * 100

        # Fill areas
        ax2.fill_between(df_pd['datetime'], 0, spread, where=(spread > 5),
                         alpha=0.6, color='red', label='Strong signal (>5pp)')
        ax2.fill_between(df_pd['datetime'], 0, spread, where=(spread > 0) & (spread <= 5),
                         alpha=0.4, color='orange', label='Moderate signal (0-5pp)')
        ax2.fill_between(df_pd['datetime'], spread, 0, where=(spread < 0),
                         alpha=0.3, color='green', label='Normal (<0pp)')

        ax2.plot(df_pd['datetime'], spread, linewidth=1.5, alpha=0.9, color='darkblue')
        ax2.axhline(y=0, color='black', linestyle='-', linewidth=1)

        # Add earnings lines to spread panel
        for i, (date, time_str) in enumerate(earnings):
            color = earnings_colors[i % len(earnings_colors)]
            ax2.axvline(x=date, color=color, linestyle='--', linewidth=2, alpha=0.8)

        ax2.set_ylabel('7d - 30d Spread (pp)', fontsize=14, fontweight='bold')
        ax2.set_xlabel('Date', fontsize=14, fontweight='bold')
        ax2.set_title('Earnings Detection: Front-Week IV Premium',
                      fontsize=14, fontweight='bold')
        ax2.legend(fontsize=10, loc='upper left', framealpha=0.95)
        ax2.grid(True, alpha=0.3, linestyle='--')

    # Format x-axis
    for ax in axes:
        ax.xaxis.set_major_formatter(mdates.DateFormatter('%b %Y'))
        ax.xaxis.set_major_locator(mdates.MonthLocator())
        ax.set_xlim(df_pd['datetime'].min(), df_pd['datetime'].max())

    plt.setp(axes[-1].xaxis.get_majorticklabels(), rotation=45, ha='right', fontsize=11)

    plt.tight_layout()

    # Save or show
    if output_file:
        plt.savefig(output_file, dpi=300, bbox_inches='tight')
        print(f"Saved plot to: {output_file}")
    else:
        input_path = Path(iv_file)
        output_path = input_path.parent / f"{input_path.stem}_with_earnings.png"
        plt.savefig(output_path, dpi=300, bbox_inches='tight')
        print(f"Saved plot to: {output_path}")

    # Print summary
    print("\n" + "=" * 80)
    print(f"{symbol} ATM IV ANALYSIS WITH EARNINGS")
    print("=" * 80)
    print(f"\nData Range: {df_pd['datetime'].min().strftime('%Y-%m-%d')} to {df_pd['datetime'].max().strftime('%Y-%m-%d')}")
    print(f"Observations: {len(df_pd)}")

    print(f"\nEarnings Events:")
    for i, (date, time_str) in enumerate(earnings, 1):
        # Find IV on earnings date (or closest before)
        mask = df_pd['datetime'].dt.date <= date.date()
        if mask.any():
            closest_row = df_pd[mask].iloc[-1]
            iv_7d = closest_row.get('cm_iv_7d', float('nan')) * 100
            iv_30d = closest_row.get('cm_iv_30d', float('nan')) * 100
            spread = (closest_row.get('cm_iv_7d', 0) - closest_row.get('cm_iv_30d', 0)) * 100
            print(f"  {i}. {date.strftime('%Y-%m-%d')} {time_str}: 7d IV={iv_7d:.1f}%, 30d IV={iv_30d:.1f}%, Spread={spread:+.1f}pp")
        else:
            print(f"  {i}. {date.strftime('%Y-%m-%d')} {time_str}: (no data)")

    # Average IVs
    print(f"\nAverage IV Levels:")
    for mat in maturities:
        col = f'cm_iv_{mat}'
        if col in df_pd.columns:
            print(f"  {mat}: {df_pd[col].mean() * 100:.1f}%")

    print("=" * 80)


def main():
    parser = argparse.ArgumentParser(
        description="Plot ATM IV time series with earnings dates as vertical lines",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
    # With inline earnings dates
    python plot_atm_iv_with_earnings.py ANGO_iv.parquet --earnings "08-01-2025 BMO, 02-04-2025 BMO, 17-07-2025 BMO, 02-10-2025 BMO"

    # With earnings from parquet file
    python plot_atm_iv_with_earnings.py ANGO_iv.parquet --earnings-file earnings_2025.parquet --output ANGO_iv_analysis.png

    # Custom maturities
    python plot_atm_iv_with_earnings.py ANGO_iv.parquet --earnings "08-01-2025 BMO" --maturities 7d,14d,30d,60d
""")

    parser.add_argument("iv_file", type=Path, help="Input IV parquet file from cs atm-iv command")
    parser.add_argument("--earnings", help='Earnings dates: "DD-MM-YYYY BMO/AMC, DD-MM-YYYY BMO/AMC, ..."')
    parser.add_argument("--earnings-file", type=Path, help="Load earnings from parquet file")
    parser.add_argument("--symbol", help="Symbol filter when loading from earnings file")
    parser.add_argument("--output", "-o", type=Path, help="Output PNG file path")
    parser.add_argument("--maturities", default="7d,14d,30d",
                        help="Comma-separated maturities to plot (default: 7d,14d,30d)")
    parser.add_argument("--no-spread", action="store_true", help="Don't show spread panel")

    args = parser.parse_args()

    # Parse earnings
    if args.earnings:
        earnings = parse_earnings_string(args.earnings)
    elif args.earnings_file:
        earnings = load_earnings_from_parquet(args.earnings_file, args.symbol)
    else:
        parser.error("Either --earnings or --earnings-file is required")

    # Parse maturities
    maturities = [m.strip() for m in args.maturities.split(",")]

    # Plot
    plot_iv_with_earnings(
        iv_file=args.iv_file,
        earnings=earnings,
        output_file=args.output,
        maturities=maturities,
        show_spread=not args.no_spread,
    )


if __name__ == "__main__":
    main()
