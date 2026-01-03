#!/usr/bin/env python3
"""
Plot Expected Move Time Series.

Panels:
1. Expected Move (%) from straddle with earnings markers
2. IV Term Structure with expected move overlay
3. Historical comparison: Expected vs Actual on earnings

Usage:
    uv run python3 plot_expected_move.py <parquet_file> [--earnings-file <file>]
"""

import polars as pl
import matplotlib.pyplot as plt
from datetime import datetime, timedelta
import sys
import argparse


def main():
    parser = argparse.ArgumentParser(description="Plot Expected Move Time Series")
    parser.add_argument("parquet_file", help="Path to ATM IV parquet file")
    parser.add_argument("--earnings-file", help="Path to earnings outcomes parquet file (optional)")
    parser.add_argument("--output", help="Output PNG file path")
    args = parser.parse_args()

    # Read main data
    df = pl.read_parquet(args.parquet_file)

    # Convert date from days since epoch to datetime
    epoch = datetime(1970, 1, 1)
    df = df.with_columns([
        pl.col("date").map_elements(
            lambda d: epoch + timedelta(days=d),
            return_dtype=pl.Datetime
        ).alias("datetime")
    ])

    df_pd = df.to_pandas()
    symbol = df["symbol"].unique()[0]

    # Create figure with 3 panels
    fig, axes = plt.subplots(3, 1, figsize=(16, 14), sharex=True)

    # Panel 1: Expected Move over time
    ax1 = axes[0]
    if "expected_move_pct" in df_pd.columns:
        ax1.plot(df_pd["datetime"], df_pd["expected_move_pct"],
                 label="Expected Move (%)", color="#e74c3c", linewidth=2)
        ax1.plot(df_pd["datetime"], df_pd["expected_move_85_pct"],
                 label="Expected Move × 0.85", color="#3498db", linewidth=2, linestyle="--")

    ax1.set_ylabel("Expected Move (%)")
    ax1.set_title(f"{symbol} - Expected Move from ATM Straddle")
    ax1.legend(loc="upper left")
    ax1.grid(True, alpha=0.3)

    # Panel 2: IV with straddle overlay
    ax2 = axes[1]
    if "cm_iv_7d" in df_pd.columns:
        ax2.plot(df_pd["datetime"], df_pd["cm_iv_7d"] * 100, label="7d IV", color="red", linewidth=1.5)
    if "cm_iv_30d" in df_pd.columns:
        ax2.plot(df_pd["datetime"], df_pd["cm_iv_30d"] * 100, label="30d IV", color="blue", linewidth=1.5)

    ax2_twin = ax2.twinx()
    if "straddle_price_nearest" in df_pd.columns:
        ax2_twin.plot(df_pd["datetime"], df_pd["straddle_price_nearest"],
                      label="Straddle $", color="green", linewidth=1.5, alpha=0.7)
        ax2_twin.set_ylabel("Straddle Price ($)", color="green")
        ax2_twin.tick_params(axis="y", labelcolor="green")

    ax2.set_ylabel("Implied Volatility (%)")
    ax2.set_title("IV and Straddle Price")
    ax2.legend(loc="upper left")
    ax2.grid(True, alpha=0.3)

    # Panel 3: If earnings outcomes available
    ax3 = axes[2]
    if args.earnings_file:
        try:
            earnings_df = pl.read_parquet(args.earnings_file)
            # Convert earnings_date from i32 to datetime
            earnings_df = earnings_df.with_columns([
                pl.col("earnings_date").map_elements(
                    lambda d: epoch + timedelta(days=d),
                    return_dtype=pl.Datetime
                ).alias("earnings_datetime")
            ])
            earnings_pd = earnings_df.to_pandas()

            ax3.scatter(earnings_pd["earnings_datetime"], earnings_pd["expected_move_pct"],
                        label="Expected", color="blue", s=100, marker="o", alpha=0.7)
            ax3.scatter(earnings_pd["earnings_datetime"], earnings_pd["actual_move_pct"],
                        label="Actual", color="red", s=100, marker="x", linewidths=2)

            # Draw lines connecting expected to actual
            for _, row in earnings_pd.iterrows():
                color = "green" if row["gamma_dominated"] else "gray"
                ax3.plot([row["earnings_datetime"], row["earnings_datetime"]],
                         [row["expected_move_pct"], row["actual_move_pct"]],
                         color=color, linewidth=2, alpha=0.6)

            ax3.axhline(y=0, color="gray", linestyle="--", alpha=0.5)
            ax3.legend(loc="upper left")
        except Exception as e:
            ax3.text(0.5, 0.5, f"Error loading earnings file:\n{str(e)}",
                     ha="center", va="center", transform=ax3.transAxes, fontsize=12)
    else:
        ax3.text(0.5, 0.5, "Earnings outcomes not provided\n(use --earnings-file)",
                 ha="center", va="center", transform=ax3.transAxes, fontsize=14, color="gray")

    ax3.set_ylabel("Move (%)")
    ax3.set_xlabel("Date")
    ax3.set_title("Expected vs Actual Moves on Earnings")
    ax3.grid(True, alpha=0.3)

    plt.tight_layout()

    # Save figure
    output = args.output or f"{args.parquet_file.replace('.parquet', '')}_expected_move.png"
    plt.savefig(output, dpi=300, bbox_inches="tight")
    print(f"✓ Saved: {output}")


if __name__ == "__main__":
    main()
