#!/usr/bin/env python3
"""
Calendar Spread Backtest Analysis

Analyzes calendar spread trades around earnings to find patterns:
1. IV term structure (short vs long IV)
2. IV vs HV comparison
3. Trade P&L by various dimensions
4. Pattern identification for profitable setups
"""

import json
import polars as pl
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path
from datetime import datetime

def load_backtest_data(json_path: str) -> pl.DataFrame:
    """Load backtest results from JSON."""
    with open(json_path) as f:
        data = json.load(f)

    # Filter for successful trades only and clean up failure_reason
    clean_data = []
    for row in data:
        if row.get("success", False):
            # Remove or normalize failure_reason
            row["failure_reason"] = None
            clean_data.append(row)

    # Convert to DataFrame
    df = pl.DataFrame(clean_data)

    # Parse dates
    df = df.with_columns([
        pl.col("earnings_date").str.to_date().alias("earnings_date"),
        pl.col("entry_time").str.to_datetime(time_zone="UTC").alias("entry_time"),
        pl.col("exit_time").str.to_datetime(time_zone="UTC").alias("exit_time"),
    ])

    # Convert numeric columns
    numeric_cols = ["strike", "short_entry_price", "long_entry_price", "entry_cost",
                    "short_exit_price", "long_exit_price", "exit_value", "pnl",
                    "pnl_per_contract", "pnl_pct"]
    for col in numeric_cols:
        if col in df.columns:
            df = df.with_columns(pl.col(col).cast(pl.Float64))

    return df


def add_derived_metrics(df: pl.DataFrame) -> pl.DataFrame:
    """Add derived metrics for analysis."""
    df = df.with_columns([
        # IV spread (term structure)
        (pl.col("iv_long_entry") - pl.col("iv_short_entry")).alias("iv_spread_entry"),
        (pl.col("iv_long_exit") - pl.col("iv_short_exit")).alias("iv_spread_exit"),

        # IV change
        (pl.col("iv_short_exit") - pl.col("iv_short_entry")).alias("iv_short_change"),
        (pl.col("iv_long_exit") - pl.col("iv_long_entry")).alias("iv_long_change"),

        # IV ratio change
        (pl.col("iv_short_entry") / pl.col("iv_long_entry")).alias("iv_ratio"),

        # Net vega P&L as percentage of entry
        (pl.col("vega_pnl").cast(pl.Float64) / pl.col("entry_cost") * 100).alias("vega_pnl_pct"),

        # Winner flag
        (pl.col("pnl") > 0).alias("is_winner"),

        # DTE calculations
        ((pl.col("exit_time") - pl.col("entry_time")).dt.total_days()).alias("holding_days"),
    ])

    return df


def analyze_by_iv_metrics(df: pl.DataFrame) -> pl.DataFrame:
    """Analyze win rate by IV metrics."""
    # Bucket by IV ratio at entry
    df = df.with_columns([
        pl.when(pl.col("iv_ratio") < 0.9).then(pl.lit("<0.9"))
          .when(pl.col("iv_ratio") < 1.0).then(pl.lit("0.9-1.0"))
          .when(pl.col("iv_ratio") < 1.1).then(pl.lit("1.0-1.1"))
          .when(pl.col("iv_ratio") < 1.2).then(pl.lit("1.1-1.2"))
          .otherwise(pl.lit(">1.2"))
          .alias("iv_ratio_bucket"),

        # IV short level buckets
        pl.when(pl.col("iv_short_entry") < 0.3).then(pl.lit("<30%"))
          .when(pl.col("iv_short_entry") < 0.5).then(pl.lit("30-50%"))
          .when(pl.col("iv_short_entry") < 0.7).then(pl.lit("50-70%"))
          .when(pl.col("iv_short_entry") < 1.0).then(pl.lit("70-100%"))
          .otherwise(pl.lit(">100%"))
          .alias("iv_short_bucket"),
    ])

    return df


def print_summary_stats(df: pl.DataFrame):
    """Print summary statistics."""
    print("=" * 70)
    print("CALENDAR SPREAD BACKTEST ANALYSIS")
    print("=" * 70)
    print()

    total = len(df)
    winners = df.filter(pl.col("is_winner")).height

    print(f"Total Trades: {total}")
    print(f"Winners: {winners} ({100*winners/total:.1f}%)")
    print(f"Losers: {total - winners} ({100*(total-winners)/total:.1f}%)")
    print()

    # P&L stats
    print("P&L Statistics:")
    print(f"  Total P&L: ${df['pnl'].sum():.2f}")
    print(f"  Avg P&L: ${df['pnl'].mean():.2f}")
    print(f"  Median P&L: ${df['pnl'].median():.2f}")
    print(f"  Std Dev: ${df['pnl'].std():.2f}")
    print()

    # P&L % stats
    print("Return Statistics:")
    print(f"  Avg Return: {df['pnl_pct'].mean():.1f}%")
    print(f"  Median Return: {df['pnl_pct'].median():.1f}%")
    print(f"  Std Dev: {df['pnl_pct'].std():.1f}%")
    print()

    # IV stats
    print("IV Statistics at Entry:")
    print(f"  Avg Short IV: {df['iv_short_entry'].mean()*100:.1f}%")
    print(f"  Avg Long IV: {df['iv_long_entry'].mean()*100:.1f}%")
    print(f"  Avg IV Ratio: {df['iv_ratio'].mean():.3f}")
    print()


def analyze_by_buckets(df: pl.DataFrame):
    """Analyze win rate by various buckets."""
    print("=" * 70)
    print("WIN RATE BY IV RATIO AT ENTRY")
    print("=" * 70)

    bucket_stats = df.group_by("iv_ratio_bucket").agg([
        pl.len().alias("count"),
        pl.col("is_winner").mean().alias("win_rate"),
        pl.col("pnl").mean().alias("avg_pnl"),
        pl.col("pnl_pct").mean().alias("avg_return"),
        pl.col("iv_short_entry").mean().alias("avg_short_iv"),
        pl.col("vega_pnl_pct").mean().alias("avg_vega_pnl_pct"),
    ]).sort("iv_ratio_bucket")

    print(bucket_stats)
    print()

    print("=" * 70)
    print("WIN RATE BY SHORT IV LEVEL")
    print("=" * 70)

    iv_stats = df.group_by("iv_short_bucket").agg([
        pl.len().alias("count"),
        pl.col("is_winner").mean().alias("win_rate"),
        pl.col("pnl").mean().alias("avg_pnl"),
        pl.col("pnl_pct").mean().alias("avg_return"),
        pl.col("iv_ratio").mean().alias("avg_iv_ratio"),
    ]).sort("iv_short_bucket")

    print(iv_stats)
    print()


def analyze_top_symbols(df: pl.DataFrame, top_n: int = 20):
    """Analyze top traded symbols."""
    print("=" * 70)
    print(f"TOP {top_n} SYMBOLS BY TRADE COUNT")
    print("=" * 70)

    symbol_stats = df.group_by("symbol").agg([
        pl.len().alias("trades"),
        pl.col("is_winner").mean().alias("win_rate"),
        pl.col("pnl").sum().alias("total_pnl"),
        pl.col("pnl").mean().alias("avg_pnl"),
        pl.col("pnl_pct").mean().alias("avg_return"),
        pl.col("iv_short_entry").mean().alias("avg_short_iv"),
        pl.col("iv_ratio").mean().alias("avg_iv_ratio"),
    ]).sort("trades", descending=True).head(top_n)

    print(symbol_stats)
    print()

    # Best performers
    print("=" * 70)
    print("BEST PERFORMING SYMBOLS (min 5 trades)")
    print("=" * 70)

    best = df.group_by("symbol").agg([
        pl.len().alias("trades"),
        pl.col("is_winner").mean().alias("win_rate"),
        pl.col("pnl").sum().alias("total_pnl"),
        pl.col("pnl_pct").mean().alias("avg_return"),
        pl.col("iv_ratio").mean().alias("avg_iv_ratio"),
    ]).filter(pl.col("trades") >= 5).sort("total_pnl", descending=True).head(10)

    print(best)
    print()


def analyze_vega_contribution(df: pl.DataFrame):
    """Analyze vega P&L contribution."""
    print("=" * 70)
    print("VEGA P&L CONTRIBUTION ANALYSIS")
    print("=" * 70)

    # Convert vega_pnl to float
    df_vega = df.with_columns([
        pl.col("vega_pnl").cast(pl.Float64).alias("vega_pnl_float"),
        pl.col("delta_pnl").cast(pl.Float64).alias("delta_pnl_float"),
        pl.col("theta_pnl").cast(pl.Float64).alias("theta_pnl_float"),
        pl.col("gamma_pnl").cast(pl.Float64).alias("gamma_pnl_float"),
    ])

    print("Greeks P&L Attribution (average per trade):")
    print(f"  Delta P&L: ${df_vega['delta_pnl_float'].mean():.4f}")
    print(f"  Gamma P&L: ${df_vega['gamma_pnl_float'].mean():.4f}")
    print(f"  Theta P&L: ${df_vega['theta_pnl_float'].mean():.4f}")
    print(f"  Vega P&L:  ${df_vega['vega_pnl_float'].mean():.4f}")
    print()

    # Vega contribution to winners vs losers
    print("Vega P&L by Outcome:")
    winners = df_vega.filter(pl.col("is_winner"))
    losers = df_vega.filter(~pl.col("is_winner"))

    print(f"  Winners Avg Vega P&L: ${winners['vega_pnl_float'].mean():.4f}")
    print(f"  Losers Avg Vega P&L:  ${losers['vega_pnl_float'].mean():.4f}")
    print()


def create_visualizations(df: pl.DataFrame, output_dir: str = "./output"):
    """Create analysis visualizations."""
    Path(output_dir).mkdir(exist_ok=True)

    fig, axes = plt.subplots(2, 3, figsize=(18, 12))
    fig.suptitle("Calendar Spread Backtest Analysis - Q4 2025", fontsize=14, fontweight='bold')

    # 1. IV Ratio vs Return scatter
    ax1 = axes[0, 0]
    iv_ratio = df["iv_ratio"].to_numpy()
    returns = df["pnl_pct"].to_numpy()
    colors = ['green' if r > 0 else 'red' for r in returns]
    ax1.scatter(iv_ratio, returns, c=colors, alpha=0.3, s=10)
    ax1.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
    ax1.axvline(x=1.0, color='blue', linestyle='--', linewidth=0.5, label='IV Ratio = 1.0')
    ax1.set_xlabel("IV Ratio (Short/Long)")
    ax1.set_ylabel("Return (%)")
    ax1.set_title("Return vs IV Ratio at Entry")
    ax1.set_ylim(-200, 500)
    ax1.legend()

    # 2. Short IV vs Return scatter
    ax2 = axes[0, 1]
    short_iv = df["iv_short_entry"].to_numpy() * 100
    ax2.scatter(short_iv, returns, c=colors, alpha=0.3, s=10)
    ax2.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
    ax2.set_xlabel("Short IV at Entry (%)")
    ax2.set_ylabel("Return (%)")
    ax2.set_title("Return vs Short IV Level")
    ax2.set_ylim(-200, 500)

    # 3. Win rate by IV ratio bucket
    ax3 = axes[0, 2]
    bucket_order = ["<0.9", "0.9-1.0", "1.0-1.1", "1.1-1.2", ">1.2"]
    bucket_stats = df.group_by("iv_ratio_bucket").agg([
        pl.col("is_winner").mean().alias("win_rate"),
        pl.len().alias("count"),
    ])

    # Sort by bucket order
    win_rates = []
    counts = []
    for bucket in bucket_order:
        row = bucket_stats.filter(pl.col("iv_ratio_bucket") == bucket)
        if row.height > 0:
            win_rates.append(row["win_rate"][0] * 100)
            counts.append(row["count"][0])
        else:
            win_rates.append(0)
            counts.append(0)

    bars = ax3.bar(bucket_order, win_rates, color='steelblue', alpha=0.7)
    ax3.axhline(y=50, color='red', linestyle='--', label='50% baseline')
    ax3.set_xlabel("IV Ratio Bucket")
    ax3.set_ylabel("Win Rate (%)")
    ax3.set_title("Win Rate by IV Ratio")
    ax3.legend()

    # Add count labels
    for bar, count in zip(bars, counts):
        ax3.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 1,
                f'n={count}', ha='center', va='bottom', fontsize=8)

    # 4. P&L distribution
    ax4 = axes[1, 0]
    pnl = df["pnl"].to_numpy()
    ax4.hist(pnl, bins=50, color='steelblue', alpha=0.7, edgecolor='black')
    ax4.axvline(x=0, color='red', linestyle='--', label='Break-even')
    ax4.axvline(x=np.mean(pnl), color='green', linestyle='-', label=f'Mean: ${np.mean(pnl):.2f}')
    ax4.set_xlabel("P&L ($)")
    ax4.set_ylabel("Frequency")
    ax4.set_title("P&L Distribution")
    ax4.legend()
    ax4.set_xlim(-5, 5)

    # 5. IV change (short vs long)
    ax5 = axes[1, 1]
    iv_short_change = df["iv_short_change"].to_numpy() * 100
    iv_long_change = df["iv_long_change"].to_numpy() * 100
    ax5.scatter(iv_short_change, iv_long_change, c=colors, alpha=0.3, s=10)
    ax5.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
    ax5.axvline(x=0, color='black', linestyle='-', linewidth=0.5)
    ax5.plot([-50, 50], [-50, 50], 'b--', alpha=0.5, label='Equal change')
    ax5.set_xlabel("Short IV Change (pp)")
    ax5.set_ylabel("Long IV Change (pp)")
    ax5.set_title("IV Change: Short vs Long Leg")
    ax5.set_xlim(-50, 50)
    ax5.set_ylim(-50, 50)
    ax5.legend()

    # 6. Greeks P&L breakdown
    ax6 = axes[1, 2]

    # Convert to float
    delta_pnl = df["delta_pnl"].cast(pl.Float64).to_numpy()
    gamma_pnl = df["gamma_pnl"].cast(pl.Float64).to_numpy()
    theta_pnl = df["theta_pnl"].cast(pl.Float64).to_numpy()
    vega_pnl = df["vega_pnl"].cast(pl.Float64).to_numpy()

    greek_means = [np.mean(delta_pnl), np.mean(gamma_pnl),
                   np.mean(theta_pnl), np.mean(vega_pnl)]
    greek_names = ['Delta', 'Gamma', 'Theta', 'Vega']
    colors_bar = ['blue', 'orange', 'purple', 'green']

    bars = ax6.bar(greek_names, greek_means, color=colors_bar, alpha=0.7)
    ax6.axhline(y=0, color='black', linestyle='-', linewidth=0.5)
    ax6.set_ylabel("Average P&L ($)")
    ax6.set_title("Greeks P&L Attribution")

    # Add value labels
    for bar, val in zip(bars, greek_means):
        ypos = bar.get_height() + 0.001 if val >= 0 else bar.get_height() - 0.003
        ax6.text(bar.get_x() + bar.get_width()/2, ypos,
                f'${val:.4f}', ha='center', va='bottom' if val >= 0 else 'top', fontsize=9)

    plt.tight_layout()
    output_path = f"{output_dir}/calendar_spread_analysis.png"
    plt.savefig(output_path, dpi=150, bbox_inches='tight')
    plt.close()

    print(f"✓ Saved visualization: {output_path}")


def find_patterns(df: pl.DataFrame):
    """Find patterns that predict profitable trades."""
    print("=" * 70)
    print("PATTERN ANALYSIS: WHAT PREDICTS WINNERS?")
    print("=" * 70)

    winners = df.filter(pl.col("is_winner"))
    losers = df.filter(~pl.col("is_winner"))

    print("\nWinners vs Losers Comparison:")
    print(f"{'Metric':<25} {'Winners':>12} {'Losers':>12} {'Diff':>12}")
    print("-" * 63)

    metrics = [
        ("Avg Short IV (%)", "iv_short_entry", 100),
        ("Avg Long IV (%)", "iv_long_entry", 100),
        ("Avg IV Ratio", "iv_ratio", 1),
        ("Avg Short IV Change (pp)", "iv_short_change", 100),
        ("Avg Long IV Change (pp)", "iv_long_change", 100),
        ("Avg Entry Cost ($)", "entry_cost", 1),
    ]

    for name, col, mult in metrics:
        w_val = winners[col].mean() * mult
        l_val = losers[col].mean() * mult
        diff = w_val - l_val
        print(f"{name:<25} {w_val:>12.2f} {l_val:>12.2f} {diff:>+12.2f}")

    print()

    # Find optimal IV ratio range
    print("=" * 70)
    print("OPTIMAL IV RATIO RANGE ANALYSIS")
    print("=" * 70)

    for low in [0.8, 0.9, 1.0, 1.1]:
        for high in [1.0, 1.1, 1.2, 1.3]:
            if low >= high:
                continue
            subset = df.filter((pl.col("iv_ratio") >= low) & (pl.col("iv_ratio") < high))
            if subset.height < 50:
                continue
            wr = subset["is_winner"].mean() * 100
            avg_ret = subset["pnl_pct"].mean()
            print(f"  IV Ratio {low:.1f}-{high:.1f}: {subset.height:4d} trades, {wr:.1f}% win rate, {avg_ret:+.1f}% avg return")

    print()


def main():
    # Load data
    df = load_backtest_data("./output/backtest_q4_2025.json")

    # Add derived metrics
    df = add_derived_metrics(df)
    df = analyze_by_iv_metrics(df)

    # Print analyses
    print_summary_stats(df)
    analyze_by_buckets(df)
    analyze_top_symbols(df)
    analyze_vega_contribution(df)
    find_patterns(df)

    # Create visualizations
    create_visualizations(df)

    print("\n" + "=" * 70)
    print("ANALYSIS COMPLETE")
    print("=" * 70)


if __name__ == "__main__":
    main()
