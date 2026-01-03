#!/usr/bin/env python3
"""
IV Evolution Analysis - Calendar Spread Setups

Analyzes how IV term structure evolves before earnings and identifies
patterns that lead to profitable calendar spread trades.
"""

import json
import polars as pl
import matplotlib.pyplot as plt
import numpy as np
from pathlib import Path
from datetime import datetime, timedelta

def load_backtest_with_iv(json_path: str) -> pl.DataFrame:
    """Load backtest results with IV data."""
    with open(json_path) as f:
        data = json.load(f)

    # Filter for successful trades
    clean_data = []
    for row in data:
        if row.get("success", False):
            row["failure_reason"] = None
            clean_data.append(row)

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


def analyze_iv_term_structure(df: pl.DataFrame):
    """Analyze IV term structure patterns."""
    print("=" * 70)
    print("IV TERM STRUCTURE ANALYSIS")
    print("=" * 70)
    print()

    # Entry IV stats
    print("Entry IV Statistics:")
    print(f"  Short Leg IV: {df['iv_short_entry'].mean()*100:.1f}% (avg), {df['iv_short_entry'].median()*100:.1f}% (median)")
    print(f"  Long Leg IV:  {df['iv_long_entry'].mean()*100:.1f}% (avg), {df['iv_long_entry'].median()*100:.1f}% (median)")
    print(f"  IV Ratio:     {df['iv_ratio_entry'].mean():.3f} (avg), {df['iv_ratio_entry'].median():.3f} (median)")
    print()

    # Exit IV stats
    print("Exit IV Statistics:")
    print(f"  Short Leg IV: {df['iv_short_exit'].mean()*100:.1f}% (avg)")
    print(f"  Long Leg IV:  {df['iv_long_exit'].mean()*100:.1f}% (avg)")
    print()

    # IV changes
    short_iv_change = (df['iv_short_exit'] - df['iv_short_entry']) * 100
    long_iv_change = (df['iv_long_exit'] - df['iv_long_entry']) * 100

    print("IV Change (Entry → Exit):")
    print(f"  Short Leg: {short_iv_change.mean():.1f}pp avg ({short_iv_change.median():.1f}pp median)")
    print(f"  Long Leg:  {long_iv_change.mean():.1f}pp avg ({long_iv_change.median():.1f}pp median)")
    print()

    # Calendar spread thesis: short crushes, long holds
    df_with_changes = df.with_columns([
        ((pl.col("iv_short_exit") - pl.col("iv_short_entry")) * 100).alias("short_iv_change"),
        ((pl.col("iv_long_exit") - pl.col("iv_long_entry")) * 100).alias("long_iv_change"),
    ])

    # Categorize by IV behavior
    df_cat = df_with_changes.with_columns([
        pl.when(
            (pl.col("short_iv_change") < -5) & (pl.col("long_iv_change") > -5)
        ).then(pl.lit("Ideal: Short crush, Long hold"))
        .when(
            (pl.col("short_iv_change") < -5) & (pl.col("long_iv_change") < -5)
        ).then(pl.lit("Both crush"))
        .when(
            (pl.col("short_iv_change") > 5)
        ).then(pl.lit("Short IV rose"))
        .otherwise(pl.lit("Other"))
        .alias("iv_behavior")
    ])

    print("IV Behavior Categories:")
    behavior_stats = df_cat.group_by("iv_behavior").agg([
        pl.len().alias("count"),
        (pl.col("pnl") > 0).mean().alias("win_rate"),
        pl.col("pnl_pct").mean().alias("avg_return"),
    ]).sort("count", descending=True)

    for row in behavior_stats.iter_rows(named=True):
        print(f"  {row['iv_behavior']}: {row['count']} trades, {row['win_rate']*100:.1f}% win rate, {row['avg_return']:.1f}% avg return")
    print()

    return df_cat


def analyze_entry_conditions(df: pl.DataFrame):
    """Analyze what entry conditions lead to good trades."""
    print("=" * 70)
    print("ENTRY CONDITIONS ANALYSIS")
    print("=" * 70)
    print()

    # Add buckets for IV ratio
    df = df.with_columns([
        pl.when(pl.col("iv_ratio_entry") < 1.0).then(pl.lit("Backwardation (<1.0)"))
          .when(pl.col("iv_ratio_entry") < 1.1).then(pl.lit("Flat (1.0-1.1)"))
          .when(pl.col("iv_ratio_entry") < 1.2).then(pl.lit("Contango (1.1-1.2)"))
          .when(pl.col("iv_ratio_entry") < 1.3).then(pl.lit("Steep (1.2-1.3)"))
          .otherwise(pl.lit("Very Steep (>1.3)"))
          .alias("term_structure"),

        pl.when(pl.col("iv_short_entry") < 0.4).then(pl.lit("Low IV (<40%)"))
          .when(pl.col("iv_short_entry") < 0.6).then(pl.lit("Medium IV (40-60%)"))
          .when(pl.col("iv_short_entry") < 0.8).then(pl.lit("High IV (60-80%)"))
          .when(pl.col("iv_short_entry") < 1.0).then(pl.lit("Very High IV (80-100%)"))
          .otherwise(pl.lit("Extreme IV (>100%)"))
          .alias("iv_level"),
    ])

    # Cross-tabulation
    print("Win Rate by Term Structure × IV Level:")
    print()

    cross_tab = df.group_by(["term_structure", "iv_level"]).agg([
        pl.len().alias("n"),
        (pl.col("pnl") > 0).mean().alias("win_rate"),
        pl.col("pnl_pct").mean().alias("avg_return"),
    ]).sort(["term_structure", "iv_level"])

    # Pivot for display
    pivot = cross_tab.pivot(
        on="iv_level",
        values="win_rate",
        index="term_structure"
    )
    print("Win Rate Matrix:")
    print(pivot)
    print()

    # Best conditions
    print("=" * 70)
    print("BEST ENTRY CONDITIONS (min 30 trades)")
    print("=" * 70)
    best = cross_tab.filter(pl.col("n") >= 30).sort("win_rate", descending=True).head(10)
    for row in best.iter_rows(named=True):
        print(f"  {row['term_structure']} + {row['iv_level']}: {row['n']} trades, {row['win_rate']*100:.1f}% win rate, {row['avg_return']:.1f}% avg return")
    print()

    return df


def analyze_greeks_pnl(df: pl.DataFrame):
    """Analyze Greeks contribution to P&L."""
    print("=" * 70)
    print("GREEKS P&L DECOMPOSITION")
    print("=" * 70)
    print()

    # Convert string columns to float
    df = df.with_columns([
        pl.col("delta_pnl").cast(pl.Float64).alias("delta_pnl"),
        pl.col("gamma_pnl").cast(pl.Float64).alias("gamma_pnl"),
        pl.col("theta_pnl").cast(pl.Float64).alias("theta_pnl"),
        pl.col("vega_pnl").cast(pl.Float64).alias("vega_pnl"),
    ])

    winners = df.filter(pl.col("pnl") > 0)
    losers = df.filter(pl.col("pnl") <= 0)

    print(f"{'Greek':<12} {'All Trades':>12} {'Winners':>12} {'Losers':>12}")
    print("-" * 52)

    for greek in ["delta", "gamma", "theta", "vega"]:
        col = f"{greek}_pnl"
        all_avg = df[col].mean()
        win_avg = winners[col].mean()
        lose_avg = losers[col].mean()
        print(f"{greek.capitalize():<12} ${all_avg:>10.4f} ${win_avg:>10.4f} ${lose_avg:>10.4f}")

    print()

    # Vega as % of total P&L
    print("Vega Contribution Analysis:")
    df_vega = df.with_columns([
        (pl.col("vega_pnl").abs() / (pl.col("pnl").abs() + 0.001) * 100).clip(0, 500).alias("vega_contribution")
    ])
    print(f"  Avg Vega Contribution to P&L: {df_vega['vega_contribution'].mean():.1f}%")
    print(f"  Median Vega Contribution: {df_vega['vega_contribution'].median():.1f}%")
    print()

    # Vega P&L correlation with win
    print("Vega P&L by Outcome:")
    print(f"  Winners: ${winners['vega_pnl'].mean():.4f} avg")
    print(f"  Losers:  ${losers['vega_pnl'].mean():.4f} avg")
    print()

    return df


def create_advanced_visualizations(df: pl.DataFrame, output_dir: str = "./output"):
    """Create advanced visualizations."""
    Path(output_dir).mkdir(exist_ok=True)

    fig, axes = plt.subplots(2, 3, figsize=(18, 12))
    fig.suptitle("Calendar Spread IV Analysis - Q4 2025", fontsize=14, fontweight='bold')

    # 1. IV Ratio Distribution (Winners vs Losers)
    ax1 = axes[0, 0]
    winners = df.filter(pl.col("pnl") > 0)
    losers = df.filter(pl.col("pnl") <= 0)

    ax1.hist(winners["iv_ratio_entry"].to_numpy(), bins=30, alpha=0.5, label=f'Winners (n={len(winners)})', color='green', density=True)
    ax1.hist(losers["iv_ratio_entry"].to_numpy(), bins=30, alpha=0.5, label=f'Losers (n={len(losers)})', color='red', density=True)
    ax1.axvline(x=1.0, color='black', linestyle='--', label='IV Ratio = 1.0')
    ax1.set_xlabel("IV Ratio at Entry (Short/Long)")
    ax1.set_ylabel("Density")
    ax1.set_title("IV Ratio Distribution: Winners vs Losers")
    ax1.legend()

    # 2. Short IV Change Distribution
    ax2 = axes[0, 1]
    short_change_w = (winners["iv_short_exit"] - winners["iv_short_entry"]).to_numpy() * 100
    short_change_l = (losers["iv_short_exit"] - losers["iv_short_entry"]).to_numpy() * 100

    ax2.hist(short_change_w, bins=30, alpha=0.5, label='Winners', color='green', density=True)
    ax2.hist(short_change_l, bins=30, alpha=0.5, label='Losers', color='red', density=True)
    ax2.axvline(x=0, color='black', linestyle='--')
    ax2.axvline(x=np.mean(short_change_w), color='green', linestyle='-', label=f'Winners Mean: {np.mean(short_change_w):.1f}pp')
    ax2.axvline(x=np.mean(short_change_l), color='red', linestyle='-', label=f'Losers Mean: {np.mean(short_change_l):.1f}pp')
    ax2.set_xlabel("Short IV Change (percentage points)")
    ax2.set_ylabel("Density")
    ax2.set_title("Short Leg IV Change: Winners vs Losers")
    ax2.legend(fontsize=8)
    ax2.set_xlim(-50, 50)

    # 3. Long IV Change Distribution
    ax3 = axes[0, 2]
    long_change_w = (winners["iv_long_exit"] - winners["iv_long_entry"]).to_numpy() * 100
    long_change_l = (losers["iv_long_exit"] - losers["iv_long_entry"]).to_numpy() * 100

    ax3.hist(long_change_w, bins=30, alpha=0.5, label='Winners', color='green', density=True)
    ax3.hist(long_change_l, bins=30, alpha=0.5, label='Losers', color='red', density=True)
    ax3.axvline(x=0, color='black', linestyle='--')
    ax3.axvline(x=np.mean(long_change_w), color='green', linestyle='-', label=f'Winners Mean: {np.mean(long_change_w):.1f}pp')
    ax3.axvline(x=np.mean(long_change_l), color='red', linestyle='-', label=f'Losers Mean: {np.mean(long_change_l):.1f}pp')
    ax3.set_xlabel("Long IV Change (percentage points)")
    ax3.set_ylabel("Density")
    ax3.set_title("Long Leg IV Change: Winners vs Losers")
    ax3.legend(fontsize=8)
    ax3.set_xlim(-50, 50)

    # 4. Win Rate Heatmap by IV Ratio and Short IV Level
    ax4 = axes[1, 0]
    df_binned = df.with_columns([
        ((pl.col("iv_ratio_entry") * 10).floor() / 10).alias("iv_ratio_bin"),
        ((pl.col("iv_short_entry") * 10).floor() / 10).alias("iv_short_bin"),
    ]).with_columns([
        pl.col("iv_ratio_bin").clip(0.5, 1.5),
        pl.col("iv_short_bin").clip(0.2, 1.2),
    ]).drop_nulls(["iv_ratio_bin", "iv_short_bin"])

    heatmap_data = df_binned.group_by(["iv_ratio_bin", "iv_short_bin"]).agg([
        (pl.col("pnl") > 0).mean().alias("win_rate"),
    ]).drop_nulls()

    # Create heatmap matrix
    iv_ratios = sorted([x for x in heatmap_data["iv_ratio_bin"].to_list() if x is not None])
    iv_shorts = sorted([x for x in heatmap_data["iv_short_bin"].to_list() if x is not None])

    matrix = np.zeros((len(iv_shorts), len(iv_ratios)))
    for row in heatmap_data.iter_rows(named=True):
        if row["iv_short_bin"] in iv_shorts and row["iv_ratio_bin"] in iv_ratios:
            i = iv_shorts.index(row["iv_short_bin"])
            j = iv_ratios.index(row["iv_ratio_bin"])
            if row["win_rate"] is not None:
                matrix[i, j] = row["win_rate"] * 100

    im = ax4.imshow(matrix, cmap='RdYlGn', aspect='auto', vmin=30, vmax=70)
    ax4.set_xticks(range(len(iv_ratios)))
    ax4.set_xticklabels([f'{x:.1f}' for x in iv_ratios], rotation=45)
    ax4.set_yticks(range(len(iv_shorts)))
    ax4.set_yticklabels([f'{int(x*100)}%' for x in iv_shorts])
    ax4.set_xlabel("IV Ratio at Entry")
    ax4.set_ylabel("Short IV at Entry")
    ax4.set_title("Win Rate Heatmap")
    plt.colorbar(im, ax=ax4, label='Win Rate (%)')

    # 5. Cumulative P&L by IV Ratio filter
    ax5 = axes[1, 1]

    for threshold in [1.0, 1.1, 1.2, 1.3]:
        subset = df.filter(pl.col("iv_ratio_entry") >= threshold).sort("earnings_date")
        if len(subset) > 10:
            cum_pnl = np.cumsum(subset["pnl"].to_numpy())
            ax5.plot(range(len(cum_pnl)), cum_pnl, label=f'IV Ratio ≥ {threshold} (n={len(subset)})')

    ax5.axhline(y=0, color='black', linestyle='--')
    ax5.set_xlabel("Trade Number")
    ax5.set_ylabel("Cumulative P&L ($)")
    ax5.set_title("Cumulative P&L by IV Ratio Filter")
    ax5.legend()

    # 6. Return Distribution by IV Ratio Bucket
    ax6 = axes[1, 2]
    buckets = ["<1.0", "1.0-1.1", "1.1-1.2", "1.2-1.3", "≥1.3"]
    bucket_data = []

    filters = [
        pl.col("iv_ratio_entry") < 1.0,
        (pl.col("iv_ratio_entry") >= 1.0) & (pl.col("iv_ratio_entry") < 1.1),
        (pl.col("iv_ratio_entry") >= 1.1) & (pl.col("iv_ratio_entry") < 1.2),
        (pl.col("iv_ratio_entry") >= 1.2) & (pl.col("iv_ratio_entry") < 1.3),
        pl.col("iv_ratio_entry") >= 1.3,
    ]

    for filt in filters:
        subset = df.filter(filt)
        bucket_data.append(subset["pnl_pct"].to_numpy())

    bp = ax6.boxplot(bucket_data, labels=buckets, patch_artist=True)
    for patch, color in zip(bp['boxes'], ['red', 'orange', 'yellow', 'lightgreen', 'green']):
        patch.set_facecolor(color)
        patch.set_alpha(0.5)

    ax6.axhline(y=0, color='black', linestyle='--')
    ax6.set_xlabel("IV Ratio Bucket")
    ax6.set_ylabel("Return (%)")
    ax6.set_title("Return Distribution by IV Ratio")
    ax6.set_ylim(-200, 400)

    plt.tight_layout()
    output_path = f"{output_dir}/iv_evolution_analysis.png"
    plt.savefig(output_path, dpi=150, bbox_inches='tight')
    plt.close()

    print(f"✓ Saved visualization: {output_path}")


def identify_ideal_setups(df: pl.DataFrame):
    """Identify ideal trade setups based on analysis."""
    print("=" * 70)
    print("IDEAL TRADE SETUP IDENTIFICATION")
    print("=" * 70)
    print()

    # Define ideal conditions based on analysis
    ideal = df.filter(
        (pl.col("iv_ratio_entry") >= 1.2) &  # Steep contango
        (pl.col("iv_short_entry") >= 0.5) &  # Elevated short IV
        (pl.col("entry_cost") > 0)  # Valid entry
    )

    print(f"Ideal Setup Criteria:")
    print(f"  - IV Ratio ≥ 1.2 (steep term structure)")
    print(f"  - Short IV ≥ 50% (elevated volatility)")
    print()

    print(f"Results:")
    print(f"  Trades matching criteria: {len(ideal)} ({100*len(ideal)/len(df):.1f}% of all trades)")
    print(f"  Win Rate: {ideal['pnl'].gt(0).mean()*100:.1f}%")
    print(f"  Avg Return: {ideal['pnl_pct'].mean():.1f}%")
    print(f"  Total P&L: ${ideal['pnl'].sum():.2f}")
    print()

    # Compare to baseline
    baseline = df.filter(pl.col("iv_ratio_entry") < 1.2)
    print(f"Comparison to Non-Ideal Setups (IV Ratio < 1.2):")
    print(f"  Trades: {len(baseline)}")
    print(f"  Win Rate: {baseline['pnl'].gt(0).mean()*100:.1f}%")
    print(f"  Avg Return: {baseline['pnl_pct'].mean():.1f}%")
    print(f"  Total P&L: ${baseline['pnl'].sum():.2f}")
    print()

    # Edge calculation
    ideal_wr = ideal['pnl'].gt(0).mean()
    baseline_wr = baseline['pnl'].gt(0).mean()
    edge = (ideal_wr - baseline_wr) * 100

    print(f"EDGE: +{edge:.1f} percentage points win rate improvement")
    print()


def main():
    print("Loading backtest data...")
    df = load_backtest_with_iv("./output/backtest_q4_2025.json")
    print(f"Loaded {len(df)} trades")
    print()

    # Run analyses
    df = analyze_iv_term_structure(df)
    df = analyze_entry_conditions(df)
    df = analyze_greeks_pnl(df)
    identify_ideal_setups(df)

    # Create visualizations
    create_advanced_visualizations(df)

    print("=" * 70)
    print("ANALYSIS COMPLETE")
    print("=" * 70)


if __name__ == "__main__":
    main()
