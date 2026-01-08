#!/usr/bin/env python3
"""
Plotter - Visualization module for simulation results.

Provides plotting functions for:
1. P&L distributions (histograms, box plots)
2. Scenario comparisons
3. Hedging effectiveness
4. Strategy comparisons

Example:
    from engine import run_quick_simulation
    from plotter import SimulationPlotter

    results = run_quick_simulation(strategy="long_call", num_sims=1000)
    plotter = SimulationPlotter(results)

    # Plot P&L distribution
    plotter.plot_pnl_distribution()

    # Compare hedging modes
    plotter.plot_hedging_comparison()

    # Save all plots
    plotter.save_all(output_dir="./plots")
"""

import numpy as np
from typing import List, Dict, Optional, Tuple
from pathlib import Path
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
from matplotlib.figure import Figure

from engine import AggregatedResults
from aggregator import ResultsAggregator


# ============================================================================
# COLOR SCHEMES
# ============================================================================

class Colors:
    """Color palette for consistent plotting."""

    # Primary colors
    BLUE = "#1f77b4"
    ORANGE = "#ff7f0e"
    GREEN = "#2ca02c"
    RED = "#d62728"
    PURPLE = "#9467bd"
    BROWN = "#8c564b"
    PINK = "#e377c2"
    GRAY = "#7f7f7f"

    # Semantic colors
    PROFIT = "#2ca02c"
    LOSS = "#d62728"
    NEUTRAL = "#7f7f7f"

    # Hedging colors
    NO_HEDGE = "#d62728"
    DAILY_HEDGE = "#2ca02c"
    WEEKLY_HEDGE = "#1f77b4"
    THRESHOLD_HEDGE = "#ff7f0e"

    # Scenario colors
    IV_EQ_RV = "#1f77b4"
    IV_GT_RV = "#d62728"
    IV_LT_RV = "#2ca02c"
    IV_CRUSH = "#ff7f0e"
    IV_SPIKE = "#9467bd"

    @classmethod
    def get_hedging_color(cls, mode: str) -> str:
        """Get color for hedging mode."""
        mapping = {
            "none": cls.NO_HEDGE,
            "daily": cls.DAILY_HEDGE,
            "weekly": cls.WEEKLY_HEDGE,
            "threshold": cls.THRESHOLD_HEDGE,
        }
        return mapping.get(mode, cls.GRAY)

    @classmethod
    def get_scenario_color(cls, scenario: str) -> str:
        """Get color for scenario."""
        scenario_lower = scenario.lower()
        if "iv = rv" in scenario_lower or "equals" in scenario_lower:
            return cls.IV_EQ_RV
        elif "iv > rv" in scenario_lower or "greater" in scenario_lower:
            return cls.IV_GT_RV
        elif "iv < rv" in scenario_lower or "less" in scenario_lower:
            return cls.IV_LT_RV
        elif "crush" in scenario_lower:
            return cls.IV_CRUSH
        elif "spike" in scenario_lower:
            return cls.IV_SPIKE
        return cls.GRAY

    @classmethod
    def palette(cls, n: int) -> List[str]:
        """Get a palette of n colors."""
        colors = [cls.BLUE, cls.ORANGE, cls.GREEN, cls.RED, cls.PURPLE,
                  cls.BROWN, cls.PINK, cls.GRAY]
        return (colors * (n // len(colors) + 1))[:n]


# ============================================================================
# SIMULATION PLOTTER
# ============================================================================

class SimulationPlotter:
    """
    Visualization tools for simulation results.

    Provides various plot types:
    - P&L distributions (histogram, KDE, box)
    - Scenario comparisons
    - Hedging effectiveness
    - Strategy performance

    Example:
        results = run_quick_simulation(strategy="long_call", num_sims=1000)
        plotter = SimulationPlotter(results)

        # Generate all standard plots
        plotter.plot_all()

        # Or individual plots
        fig = plotter.plot_pnl_distribution()
        plt.show()
    """

    def __init__(
        self,
        results: List[AggregatedResults],
        figsize: Tuple[int, int] = (12, 8),
        style: str = "seaborn-v0_8-whitegrid",
    ):
        """
        Initialize plotter.

        Args:
            results: List of AggregatedResults from SimulationEngine
            figsize: Default figure size (width, height)
            style: Matplotlib style name
        """
        self.results = results
        self.figsize = figsize
        self.aggregator = ResultsAggregator(results)

        try:
            plt.style.use(style)
        except Exception:
            # Fallback if style not available
            pass

    def plot_pnl_distribution(
        self,
        results: Optional[List[AggregatedResults]] = None,
        title: Optional[str] = None,
        bins: int = 50,
        show_stats: bool = True,
        alpha: float = 0.6,
    ) -> Figure:
        """
        Plot P&L distribution histogram.

        Args:
            results: Results to plot (default: all)
            title: Plot title
            bins: Number of histogram bins
            show_stats: Show mean/median lines
            alpha: Histogram transparency

        Returns:
            Matplotlib Figure
        """
        results = results or self.results
        n_results = len(results)

        fig, axes = plt.subplots(1, n_results, figsize=(self.figsize[0], self.figsize[1] // 2 * n_results))
        if n_results == 1:
            axes = [axes]

        for ax, result in zip(axes, results):
            pnls = result.pnls

            # Histogram
            color = Colors.get_hedging_color(result.hedging_mode)
            ax.hist(pnls, bins=bins, color=color, alpha=alpha, edgecolor='black', linewidth=0.5)

            # Stats lines
            if show_stats:
                ax.axvline(result.mean_pnl, color='red', linestyle='--', linewidth=2, label=f'Mean: ${result.mean_pnl:.2f}')
                ax.axvline(result.median_pnl, color='blue', linestyle='--', linewidth=2, label=f'Median: ${result.median_pnl:.2f}')
                ax.axvline(0, color='black', linestyle='-', linewidth=1, alpha=0.5)

            ax.set_xlabel('P&L ($)')
            ax.set_ylabel('Frequency')
            ax.set_title(f'{result.strategy_name}\n{result.scenario_name} | {result.hedging_mode}')
            ax.legend(loc='upper right', fontsize=8)

        fig.suptitle(title or 'P&L Distribution', fontsize=14, fontweight='bold')
        plt.tight_layout()
        return fig

    def plot_pnl_comparison(
        self,
        group_by: str = "hedging",
        title: Optional[str] = None,
        bins: int = 50,
    ) -> Figure:
        """
        Plot P&L distributions grouped by category.

        Args:
            group_by: "hedging", "scenario", or "strategy"
            title: Plot title
            bins: Number of histogram bins

        Returns:
            Matplotlib Figure
        """
        pnls_by_group = self.aggregator.get_pnls_by_group(group_by)

        fig, ax = plt.subplots(figsize=self.figsize)

        colors = Colors.palette(len(pnls_by_group))

        for (name, pnls), color in zip(pnls_by_group.items(), colors):
            ax.hist(pnls, bins=bins, alpha=0.5, label=name, color=color, edgecolor='black', linewidth=0.3)

        ax.axvline(0, color='black', linestyle='-', linewidth=2, alpha=0.7)
        ax.set_xlabel('P&L ($)', fontsize=12)
        ax.set_ylabel('Frequency', fontsize=12)
        ax.set_title(title or f'P&L Distribution by {group_by.title()}', fontsize=14, fontweight='bold')
        ax.legend(loc='upper right')
        plt.tight_layout()
        return fig

    def plot_boxplot(
        self,
        group_by: str = "hedging",
        title: Optional[str] = None,
        show_outliers: bool = True,
    ) -> Figure:
        """
        Plot box plots comparing P&L distributions.

        Args:
            group_by: "hedging", "scenario", or "strategy"
            title: Plot title
            show_outliers: Whether to show outlier points

        Returns:
            Matplotlib Figure
        """
        pnls_by_group = self.aggregator.get_pnls_by_group(group_by)

        fig, ax = plt.subplots(figsize=self.figsize)

        data = list(pnls_by_group.values())
        labels = list(pnls_by_group.keys())

        bp = ax.boxplot(
            data,
            labels=labels,
            patch_artist=True,
            showfliers=show_outliers,
            flierprops=dict(marker='o', markerfacecolor='gray', markersize=4, alpha=0.5),
        )

        # Color the boxes
        colors = Colors.palette(len(data))
        for patch, color in zip(bp['boxes'], colors):
            patch.set_facecolor(color)
            patch.set_alpha(0.7)

        ax.axhline(0, color='black', linestyle='--', linewidth=1, alpha=0.7)
        ax.set_ylabel('P&L ($)', fontsize=12)
        ax.set_xlabel(group_by.title(), fontsize=12)
        ax.set_title(title or f'P&L Box Plot by {group_by.title()}', fontsize=14, fontweight='bold')
        plt.tight_layout()
        return fig

    def plot_hedging_comparison(self, scenario: Optional[str] = None) -> Figure:
        """
        Plot hedged vs unhedged P&L comparison.

        Args:
            scenario: Specific scenario to plot (default: first scenario)

        Returns:
            Matplotlib Figure
        """
        if scenario is None:
            scenario = self.aggregator.scenarios[0]

        scenario_results = self.aggregator.by_scenario.get(scenario, [])

        fig, axes = plt.subplots(1, 2, figsize=self.figsize)

        # Histogram comparison
        ax1 = axes[0]
        for result in scenario_results:
            color = Colors.get_hedging_color(result.hedging_mode)
            ax1.hist(result.pnls, bins=50, alpha=0.5, label=result.hedging_mode, color=color, edgecolor='black', linewidth=0.3)

        ax1.axvline(0, color='black', linestyle='-', linewidth=1, alpha=0.7)
        ax1.set_xlabel('P&L ($)')
        ax1.set_ylabel('Frequency')
        ax1.set_title('P&L Distribution')
        ax1.legend()

        # Box plot comparison
        ax2 = axes[1]
        data = [r.pnls for r in scenario_results]
        labels = [r.hedging_mode for r in scenario_results]
        colors = [Colors.get_hedging_color(r.hedging_mode) for r in scenario_results]

        bp = ax2.boxplot(data, labels=labels, patch_artist=True)
        for patch, color in zip(bp['boxes'], colors):
            patch.set_facecolor(color)
            patch.set_alpha(0.7)

        ax2.axhline(0, color='black', linestyle='--', linewidth=1, alpha=0.7)
        ax2.set_ylabel('P&L ($)')
        ax2.set_title('P&L Box Plot')

        fig.suptitle(f'Hedging Comparison: {scenario}', fontsize=14, fontweight='bold')
        plt.tight_layout()
        return fig

    def plot_scenario_comparison(self, hedging_mode: str = "none") -> Figure:
        """
        Plot P&L across different scenarios.

        Args:
            hedging_mode: Hedging mode to compare (default: "none")

        Returns:
            Matplotlib Figure
        """
        hedging_results = self.aggregator.by_hedging.get(hedging_mode, [])

        fig, axes = plt.subplots(1, 2, figsize=self.figsize)

        # Histogram comparison
        ax1 = axes[0]
        for result in hedging_results:
            color = Colors.get_scenario_color(result.scenario_name)
            ax1.hist(result.pnls, bins=50, alpha=0.5, label=result.scenario_name, color=color, edgecolor='black', linewidth=0.3)

        ax1.axvline(0, color='black', linestyle='-', linewidth=1, alpha=0.7)
        ax1.set_xlabel('P&L ($)')
        ax1.set_ylabel('Frequency')
        ax1.set_title('P&L Distribution')
        ax1.legend(fontsize=8)

        # Summary bar chart
        ax2 = axes[1]
        scenarios = [r.scenario_name for r in hedging_results]
        means = [r.mean_pnl for r in hedging_results]
        colors = [Colors.get_scenario_color(s) for s in scenarios]

        bars = ax2.bar(range(len(scenarios)), means, color=colors, alpha=0.7, edgecolor='black')
        ax2.set_xticks(range(len(scenarios)))
        ax2.set_xticklabels(scenarios, rotation=45, ha='right', fontsize=8)
        ax2.axhline(0, color='black', linestyle='-', linewidth=1)
        ax2.set_ylabel('Mean P&L ($)')
        ax2.set_title('Mean P&L by Scenario')

        # Add value labels on bars
        for bar, val in zip(bars, means):
            height = bar.get_height()
            ax2.annotate(
                f'${val:.2f}',
                xy=(bar.get_x() + bar.get_width() / 2, height),
                xytext=(0, 3 if height >= 0 else -10),
                textcoords="offset points",
                ha='center', va='bottom' if height >= 0 else 'top',
                fontsize=8,
            )

        fig.suptitle(f'Scenario Comparison (Hedging: {hedging_mode})', fontsize=14, fontweight='bold')
        plt.tight_layout()
        return fig

    def plot_summary_metrics(self) -> Figure:
        """
        Plot summary metrics for all configurations.

        Returns:
            Matplotlib Figure with multiple metrics subplots
        """
        fig, axes = plt.subplots(2, 2, figsize=(self.figsize[0], self.figsize[1] + 4))

        # Prepare data
        labels = [f"{r.scenario_name[:15]}\n{r.hedging_mode}" for r in self.results]
        colors = [Colors.get_hedging_color(r.hedging_mode) for r in self.results]

        # Mean P&L
        ax1 = axes[0, 0]
        means = [r.mean_pnl for r in self.results]
        bars = ax1.bar(range(len(labels)), means, color=colors, alpha=0.7, edgecolor='black')
        ax1.axhline(0, color='black', linestyle='-', linewidth=1)
        ax1.set_xticks(range(len(labels)))
        ax1.set_xticklabels(labels, rotation=45, ha='right', fontsize=7)
        ax1.set_ylabel('Mean P&L ($)')
        ax1.set_title('Mean P&L')

        # Win Rate
        ax2 = axes[0, 1]
        win_rates = [r.win_rate * 100 for r in self.results]
        bars = ax2.bar(range(len(labels)), win_rates, color=colors, alpha=0.7, edgecolor='black')
        ax2.axhline(50, color='black', linestyle='--', linewidth=1, alpha=0.7)
        ax2.set_xticks(range(len(labels)))
        ax2.set_xticklabels(labels, rotation=45, ha='right', fontsize=7)
        ax2.set_ylabel('Win Rate (%)')
        ax2.set_title('Win Rate')

        # Sharpe Ratio
        ax3 = axes[1, 0]
        sharpes = [r.sharpe_ratio for r in self.results]
        bars = ax3.bar(range(len(labels)), sharpes, color=colors, alpha=0.7, edgecolor='black')
        ax3.axhline(0, color='black', linestyle='-', linewidth=1)
        ax3.set_xticks(range(len(labels)))
        ax3.set_xticklabels(labels, rotation=45, ha='right', fontsize=7)
        ax3.set_ylabel('Sharpe Ratio')
        ax3.set_title('Sharpe Ratio')

        # Standard Deviation
        ax4 = axes[1, 1]
        stds = [r.std_pnl for r in self.results]
        bars = ax4.bar(range(len(labels)), stds, color=colors, alpha=0.7, edgecolor='black')
        ax4.set_xticks(range(len(labels)))
        ax4.set_xticklabels(labels, rotation=45, ha='right', fontsize=7)
        ax4.set_ylabel('Std Dev ($)')
        ax4.set_title('P&L Volatility')

        fig.suptitle('Summary Metrics', fontsize=14, fontweight='bold')
        plt.tight_layout()
        return fig

    def plot_cumulative_distribution(
        self,
        group_by: str = "hedging",
        title: Optional[str] = None,
    ) -> Figure:
        """
        Plot cumulative distribution functions (CDF).

        Args:
            group_by: "hedging", "scenario", or "strategy"
            title: Plot title

        Returns:
            Matplotlib Figure
        """
        pnls_by_group = self.aggregator.get_pnls_by_group(group_by)

        fig, ax = plt.subplots(figsize=self.figsize)

        colors = Colors.palette(len(pnls_by_group))

        for (name, pnls), color in zip(pnls_by_group.items(), colors):
            sorted_pnls = np.sort(pnls)
            cumulative = np.arange(1, len(sorted_pnls) + 1) / len(sorted_pnls)
            ax.plot(sorted_pnls, cumulative, label=name, color=color, linewidth=2)

        ax.axvline(0, color='black', linestyle='--', linewidth=1, alpha=0.7)
        ax.axhline(0.5, color='gray', linestyle=':', linewidth=1, alpha=0.7)
        ax.set_xlabel('P&L ($)', fontsize=12)
        ax.set_ylabel('Cumulative Probability', fontsize=12)
        ax.set_title(title or f'Cumulative Distribution by {group_by.title()}', fontsize=14, fontweight='bold')
        ax.legend(loc='lower right')
        ax.grid(True, alpha=0.3)
        plt.tight_layout()
        return fig

    def plot_percentile_comparison(self) -> Figure:
        """
        Plot percentile comparison across configurations.

        Returns:
            Matplotlib Figure
        """
        fig, ax = plt.subplots(figsize=self.figsize)

        x = np.arange(len(self.results))
        width = 0.15

        # Percentile data
        p5 = [r.pnl_5th for r in self.results]
        p25 = [r.pnl_25th for r in self.results]
        p50 = [r.median_pnl for r in self.results]
        p75 = [r.pnl_75th for r in self.results]
        p95 = [r.pnl_95th for r in self.results]

        ax.bar(x - 2*width, p5, width, label='5th %', color=Colors.RED, alpha=0.7)
        ax.bar(x - width, p25, width, label='25th %', color=Colors.ORANGE, alpha=0.7)
        ax.bar(x, p50, width, label='Median', color=Colors.GREEN, alpha=0.7)
        ax.bar(x + width, p75, width, label='75th %', color=Colors.BLUE, alpha=0.7)
        ax.bar(x + 2*width, p95, width, label='95th %', color=Colors.PURPLE, alpha=0.7)

        labels = [f"{r.scenario_name[:10]}\n{r.hedging_mode}" for r in self.results]
        ax.set_xticks(x)
        ax.set_xticklabels(labels, rotation=45, ha='right', fontsize=8)
        ax.axhline(0, color='black', linestyle='-', linewidth=1)
        ax.set_ylabel('P&L ($)')
        ax.set_title('P&L Percentiles by Configuration', fontsize=14, fontweight='bold')
        ax.legend(loc='upper right', fontsize=8)
        plt.tight_layout()
        return fig

    def plot_all(self, output_dir: Optional[Path] = None, show: bool = True) -> Dict[str, Figure]:
        """
        Generate all standard plots.

        Args:
            output_dir: Directory to save plots (optional)
            show: Whether to display plots

        Returns:
            Dict mapping plot name to Figure
        """
        plots = {
            "pnl_distribution": self.plot_pnl_distribution(),
            "hedging_comparison": self.plot_hedging_comparison() if len(self.aggregator.hedging_modes) > 1 else None,
            "scenario_comparison": self.plot_scenario_comparison() if len(self.aggregator.scenarios) > 1 else None,
            "summary_metrics": self.plot_summary_metrics(),
            "cumulative_distribution": self.plot_cumulative_distribution(),
            "boxplot": self.plot_boxplot(),
        }

        # Remove None entries
        plots = {k: v for k, v in plots.items() if v is not None}

        if output_dir:
            output_dir = Path(output_dir)
            output_dir.mkdir(parents=True, exist_ok=True)
            for name, fig in plots.items():
                fig.savefig(output_dir / f"{name}.png", dpi=150, bbox_inches='tight')

        if show:
            plt.show()

        return plots

    def save_all(self, output_dir: Path, format: str = "png", dpi: int = 150) -> List[Path]:
        """
        Save all plots to directory.

        Args:
            output_dir: Directory to save plots
            format: Image format (png, pdf, svg)
            dpi: Resolution

        Returns:
            List of saved file paths
        """
        output_dir = Path(output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)

        saved_paths = []
        plots = self.plot_all(show=False)

        for name, fig in plots.items():
            path = output_dir / f"{name}.{format}"
            fig.savefig(path, dpi=dpi, bbox_inches='tight')
            saved_paths.append(path)
            plt.close(fig)

        return saved_paths


# ============================================================================
# CONVENIENCE FUNCTIONS
# ============================================================================

def quick_plot(
    results: List[AggregatedResults],
    plot_type: str = "distribution",
    **kwargs
) -> Figure:
    """
    Quick plotting function.

    Args:
        results: Simulation results
        plot_type: "distribution", "comparison", "boxplot", "summary", "cdf"
        **kwargs: Additional arguments for the plot function

    Returns:
        Matplotlib Figure
    """
    plotter = SimulationPlotter(results)

    plot_funcs = {
        "distribution": plotter.plot_pnl_distribution,
        "comparison": plotter.plot_pnl_comparison,
        "boxplot": plotter.plot_boxplot,
        "summary": plotter.plot_summary_metrics,
        "cdf": plotter.plot_cumulative_distribution,
        "hedging": plotter.plot_hedging_comparison,
        "scenario": plotter.plot_scenario_comparison,
    }

    func = plot_funcs.get(plot_type, plotter.plot_pnl_distribution)
    return func(**kwargs)
