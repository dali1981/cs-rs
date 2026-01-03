#!/usr/bin/env python3
"""
Generate comprehensive earnings analysis report.

Outputs:
1. Summary statistics table
2. Scatter plot: Expected vs Actual
3. Move ratio histogram
4. IV crush distribution
5. Win rate by expected move size

Usage:
    uv run python3 earnings_analysis_report.py <earnings_outcomes.parquet>
"""

import polars as pl
import matplotlib.pyplot as plt
import numpy as np
import sys


def main():
    if len(sys.argv) < 2:
        print("Usage: python earnings_analysis_report.py <earnings_outcomes.parquet>")
        sys.exit(1)

    file = sys.argv[1]
    df = pl.read_parquet(file)

    # Convert to pandas for easier plotting
    df_pd = df.to_pandas()

    print("=" * 60)
    print("EARNINGS ANALYSIS REPORT")
    print("=" * 60)

    # Summary stats
    n = len(df_pd)
    if n == 0:
        print("\nNo earnings events found in the file.")
        return

    gamma_wins = df_pd["gamma_dominated"].sum()
    vega_wins = n - gamma_wins

    print(f"\nTotal Earnings Events: {n}")
    print(f"Gamma Dominated (Actual > Expected): {gamma_wins} ({100*gamma_wins/n:.1f}%)")
    print(f"Vega Dominated  (Actual < Expected): {vega_wins} ({100*vega_wins/n:.1f}%)")

    print(f"\nAverage Expected Move: {df_pd['expected_move_pct'].mean():.2f}%")
    print(f"Average Actual Move:   {df_pd['actual_move_pct'].mean():.2f}%")
    print(f"Average Move Ratio:    {df_pd['move_ratio'].mean():.2f}x")

    if "iv_crush_pct" in df_pd.columns:
        crush_data = df_pd["iv_crush_pct"].dropna()
        if len(crush_data) > 0:
            print(f"Average IV Crush:      {crush_data.mean()*100:.1f}%")

    # Create visualization
    fig, axes = plt.subplots(2, 2, figsize=(14, 12))

    # 1. Scatter: Expected vs Actual
    ax1 = axes[0, 0]
    colors = ["green" if g else "red" for g in df_pd["gamma_dominated"]]
    ax1.scatter(df_pd["expected_move_pct"], df_pd["actual_move_pct"], c=colors, alpha=0.6, s=50)
    max_val = max(df_pd["expected_move_pct"].max(), df_pd["actual_move_pct"].max())
    ax1.plot([0, max_val], [0, max_val], "k--", label="Expected = Actual", linewidth=1.5)
    ax1.set_xlabel("Expected Move (%)")
    ax1.set_ylabel("Actual Move (%)")
    ax1.set_title("Expected vs Actual Earnings Moves")
    ax1.legend()
    ax1.grid(True, alpha=0.3)

    # 2. Move ratio histogram
    ax2 = axes[0, 1]
    ax2.hist(df_pd["move_ratio"], bins=20, color="steelblue", edgecolor="black", alpha=0.7)
    ax2.axvline(x=1.0, color="red", linestyle="--", linewidth=2, label="Break-even")
    mean_ratio = df_pd["move_ratio"].mean()
    ax2.axvline(x=mean_ratio, color="green", linewidth=2, label=f"Mean: {mean_ratio:.2f}")
    ax2.set_xlabel("Move Ratio (Actual / Expected)")
    ax2.set_ylabel("Frequency")
    ax2.set_title("Move Ratio Distribution")
    ax2.legend()
    ax2.grid(True, alpha=0.3)

    # 3. IV Crush distribution
    ax3 = axes[1, 0]
    if "iv_crush_pct" in df_pd.columns:
        crush = df_pd["iv_crush_pct"].dropna() * 100
        if len(crush) > 0:
            ax3.hist(crush, bins=20, color="coral", edgecolor="black", alpha=0.7)
            ax3.axvline(x=crush.mean(), color="blue", linewidth=2, label=f"Mean: {crush.mean():.1f}%")
            ax3.set_xlabel("IV Crush (%)")
            ax3.set_ylabel("Frequency")
            ax3.set_title("IV Crush Distribution")
            ax3.legend()
            ax3.grid(True, alpha=0.3)
        else:
            ax3.text(0.5, 0.5, "No IV crush data available",
                     ha="center", va="center", transform=ax3.transAxes, fontsize=12)
    else:
        ax3.text(0.5, 0.5, "No IV crush data available",
                 ha="center", va="center", transform=ax3.transAxes, fontsize=12)

    # 4. Win rate by expected move size
    ax4 = axes[1, 1]
    try:
        # Create buckets for expected move
        import pandas as pd
        df_pd["expected_bucket"] = pd.cut(
            df_pd["expected_move_pct"],
            bins=[0, 3, 5, 7, 10, 100],
            labels=["0-3%", "3-5%", "5-7%", "7-10%", "10%+"]
        )
        win_rates = df_pd.groupby("expected_bucket", observed=True)["gamma_dominated"].mean() * 100
        win_rates.plot(kind="bar", ax=ax4, color="teal", edgecolor="black")
        ax4.axhline(y=50, color="red", linestyle="--", label="50% (Random)")
        ax4.set_xlabel("Expected Move Range")
        ax4.set_ylabel("Gamma Win Rate (%)")
        ax4.set_title("Gamma Win Rate by Expected Move Size")
        ax4.set_xticklabels(ax4.get_xticklabels(), rotation=45)
        ax4.legend()
        ax4.grid(True, alpha=0.3, axis="y")
    except Exception as e:
        ax4.text(0.5, 0.5, f"Could not compute win rates:\n{str(e)}",
                 ha="center", va="center", transform=ax4.transAxes, fontsize=10)

    plt.tight_layout()
    output = file.replace(".parquet", "_report.png")
    plt.savefig(output, dpi=300, bbox_inches="tight")
    print(f"\n✓ Saved visualization: {output}")


if __name__ == "__main__":
    import pandas as pd  # Import here for pd.cut
    main()
