#!/usr/bin/env python3
"""
Results Aggregator - Compute distributions and statistics across simulation results.

This module provides tools for:
1. Aggregating results from multiple simulations
2. Computing P&L distributions
3. Comparing strategies, scenarios, and hedging modes
4. Generating summary reports

Example:
    from engine import run_quick_simulation
    from aggregator import ResultsAggregator

    results = run_quick_simulation(strategy="long_call", num_sims=1000)
    agg = ResultsAggregator(results)

    # Compare hedging vs no hedging
    comparison = agg.compare_hedging()
    print(comparison)

    # Get full distribution stats
    stats = agg.distribution_stats()
"""

import numpy as np
from dataclasses import dataclass, field
from typing import List, Dict, Optional, Tuple
from collections import defaultdict

from engine import AggregatedResults


# ============================================================================
# COMPARISON RESULTS
# ============================================================================

@dataclass
class ComparisonResult:
    """Result of comparing two configurations."""
    name_a: str
    name_b: str
    metric: str

    value_a: float
    value_b: float
    difference: float
    pct_difference: float

    def __str__(self) -> str:
        arrow = "+" if self.difference > 0 else ""
        return (
            f"{self.metric}: {self.name_a}={self.value_a:.2f} vs "
            f"{self.name_b}={self.value_b:.2f} "
            f"({arrow}{self.difference:.2f}, {arrow}{self.pct_difference:.1f}%)"
        )


@dataclass
class StrategyComparison:
    """Comprehensive comparison between two strategy configurations."""
    config_a: str
    config_b: str
    comparisons: List[ComparisonResult]

    def summary(self) -> str:
        """Generate summary string."""
        lines = [
            f"\n{'='*70}",
            f"Comparison: {self.config_a} vs {self.config_b}",
            f"{'='*70}",
        ]
        for comp in self.comparisons:
            lines.append(str(comp))
        return "\n".join(lines)


# ============================================================================
# DISTRIBUTION STATISTICS
# ============================================================================

@dataclass
class DistributionStats:
    """Detailed distribution statistics."""
    name: str
    n: int

    # Central tendency
    mean: float
    median: float
    mode_approx: float

    # Dispersion
    std: float
    variance: float
    iqr: float  # Interquartile range
    range: float

    # Shape
    skewness: float
    kurtosis: float

    # Percentiles
    p1: float
    p5: float
    p10: float
    p25: float
    p50: float
    p75: float
    p90: float
    p95: float
    p99: float

    # Extremes
    min: float
    max: float

    # Risk metrics
    var_95: float  # Value at Risk (5th percentile)
    cvar_95: float  # Conditional VaR (expected shortfall)
    probability_of_loss: float
    expected_loss: float
    expected_gain: float

    def summary(self) -> str:
        """Generate formatted summary."""
        return f"""
Distribution Statistics: {self.name}
{'─'*60}
Count: {self.n:,}

Central Tendency:
  Mean:     ${self.mean:>10.2f}
  Median:   ${self.median:>10.2f}
  Mode:     ${self.mode_approx:>10.2f}

Dispersion:
  Std Dev:  ${self.std:>10.2f}
  Variance: ${self.variance:>10.2f}
  IQR:      ${self.iqr:>10.2f}
  Range:    ${self.range:>10.2f}

Shape:
  Skewness: {self.skewness:>10.2f}  {'(right-skewed)' if self.skewness > 0 else '(left-skewed)' if self.skewness < 0 else '(symmetric)'}
  Kurtosis: {self.kurtosis:>10.2f}  {'(fat tails)' if self.kurtosis > 0 else '(thin tails)' if self.kurtosis < 0 else '(normal)'}

Percentiles:
   1%: ${self.p1:>8.2f}  │  99%: ${self.p99:>8.2f}
   5%: ${self.p5:>8.2f}  │  95%: ${self.p95:>8.2f}
  10%: ${self.p10:>8.2f}  │  90%: ${self.p90:>8.2f}
  25%: ${self.p25:>8.2f}  │  75%: ${self.p75:>8.2f}
  50%: ${self.p50:>8.2f}  (median)

Extremes:
  Min: ${self.min:>10.2f}
  Max: ${self.max:>10.2f}

Risk Metrics:
  VaR (95%):           ${self.var_95:>10.2f}  (5th percentile)
  CVaR (95%):          ${self.cvar_95:>10.2f}  (expected shortfall)
  P(Loss):             {self.probability_of_loss:>10.1%}
  E[Loss|Loss]:        ${self.expected_loss:>10.2f}
  E[Gain|Gain]:        ${self.expected_gain:>10.2f}
"""


# ============================================================================
# RESULTS AGGREGATOR
# ============================================================================

class ResultsAggregator:
    """
    Aggregates and analyzes simulation results.

    Provides methods for:
    - Computing detailed distribution statistics
    - Comparing scenarios, strategies, and hedging modes
    - Generating reports

    Example:
        results = run_quick_simulation(strategy="long_call", num_sims=1000)
        agg = ResultsAggregator(results)

        # Get distribution stats for each result
        for stats in agg.distribution_stats():
            print(stats.summary())

        # Compare hedging modes
        comparison = agg.compare_hedging()
        print(comparison)
    """

    def __init__(self, results: List[AggregatedResults]):
        """
        Initialize aggregator with simulation results.

        Args:
            results: List of AggregatedResults from SimulationEngine
        """
        self.results = results
        self._build_indices()

    def _build_indices(self):
        """Build indices for fast lookup."""
        self.by_scenario: Dict[str, List[AggregatedResults]] = defaultdict(list)
        self.by_hedging: Dict[str, List[AggregatedResults]] = defaultdict(list)
        self.by_strategy: Dict[str, List[AggregatedResults]] = defaultdict(list)

        for r in self.results:
            self.by_scenario[r.scenario_name].append(r)
            self.by_hedging[r.hedging_mode].append(r)
            self.by_strategy[r.strategy_name].append(r)

    @property
    def scenarios(self) -> List[str]:
        """List of unique scenario names."""
        return list(self.by_scenario.keys())

    @property
    def hedging_modes(self) -> List[str]:
        """List of unique hedging modes."""
        return list(self.by_hedging.keys())

    @property
    def strategies(self) -> List[str]:
        """List of unique strategy names."""
        return list(self.by_strategy.keys())

    def distribution_stats(self, result: Optional[AggregatedResults] = None) -> List[DistributionStats]:
        """
        Compute detailed distribution statistics.

        Args:
            result: Specific result to analyze, or None for all results

        Returns:
            List of DistributionStats
        """
        results_to_analyze = [result] if result else self.results

        stats_list = []
        for r in results_to_analyze:
            stats = self._compute_distribution_stats(r)
            stats_list.append(stats)

        return stats_list

    def _compute_distribution_stats(self, result: AggregatedResults) -> DistributionStats:
        """Compute distribution statistics for a single result."""
        pnls = result.pnls

        # Central tendency
        mean = float(np.mean(pnls))
        median = float(np.median(pnls))

        # Approximate mode using histogram
        hist, edges = np.histogram(pnls, bins=50)
        mode_bin = np.argmax(hist)
        mode_approx = (edges[mode_bin] + edges[mode_bin + 1]) / 2

        # Dispersion
        std = float(np.std(pnls))
        variance = float(np.var(pnls))
        iqr = float(np.percentile(pnls, 75) - np.percentile(pnls, 25))
        range_val = float(np.max(pnls) - np.min(pnls))

        # Shape
        skewness = self._compute_skewness(pnls)
        kurtosis = self._compute_kurtosis(pnls)

        # Percentiles
        percentiles = [1, 5, 10, 25, 50, 75, 90, 95, 99]
        pct_values = [float(np.percentile(pnls, p)) for p in percentiles]

        # Risk metrics
        var_95 = float(np.percentile(pnls, 5))  # 5th percentile
        losses = pnls[pnls < var_95]
        cvar_95 = float(np.mean(losses)) if len(losses) > 0 else var_95

        losers = pnls[pnls < 0]
        winners = pnls[pnls > 0]
        prob_loss = len(losers) / len(pnls)
        expected_loss = float(np.mean(losers)) if len(losers) > 0 else 0
        expected_gain = float(np.mean(winners)) if len(winners) > 0 else 0

        return DistributionStats(
            name=f"{result.strategy_name} | {result.scenario_name} | {result.hedging_mode}",
            n=len(pnls),
            mean=mean,
            median=median,
            mode_approx=mode_approx,
            std=std,
            variance=variance,
            iqr=iqr,
            range=range_val,
            skewness=skewness,
            kurtosis=kurtosis,
            p1=pct_values[0],
            p5=pct_values[1],
            p10=pct_values[2],
            p25=pct_values[3],
            p50=pct_values[4],
            p75=pct_values[5],
            p90=pct_values[6],
            p95=pct_values[7],
            p99=pct_values[8],
            min=float(np.min(pnls)),
            max=float(np.max(pnls)),
            var_95=var_95,
            cvar_95=cvar_95,
            probability_of_loss=prob_loss,
            expected_loss=expected_loss,
            expected_gain=expected_gain,
        )

    def _compute_skewness(self, data: np.ndarray) -> float:
        """Compute skewness."""
        n = len(data)
        mean = np.mean(data)
        std = np.std(data)
        if std == 0:
            return 0.0
        return float((n / ((n - 1) * (n - 2))) * np.sum(((data - mean) / std) ** 3))

    def _compute_kurtosis(self, data: np.ndarray) -> float:
        """Compute excess kurtosis."""
        n = len(data)
        mean = np.mean(data)
        std = np.std(data)
        if std == 0:
            return 0.0
        m4 = np.mean((data - mean) ** 4)
        return float(m4 / (std ** 4) - 3)

    def compare_hedging(self) -> List[StrategyComparison]:
        """
        Compare hedged vs unhedged results.

        Returns:
            List of StrategyComparison objects
        """
        comparisons = []

        # Group results by scenario
        for scenario in self.scenarios:
            scenario_results = self.by_scenario[scenario]

            # Find hedged and unhedged
            unhedged = [r for r in scenario_results if r.hedging_mode == "none"]
            hedged = [r for r in scenario_results if r.hedging_mode != "none"]

            for uh in unhedged:
                for h in hedged:
                    comp = self._compare_results(uh, h)
                    comparisons.append(comp)

        return comparisons

    def compare_scenarios(self) -> List[StrategyComparison]:
        """
        Compare results across different scenarios.

        Returns:
            List of StrategyComparison objects
        """
        comparisons = []

        # Group by hedging mode
        for hedging in self.hedging_modes:
            hedging_results = self.by_hedging[hedging]

            # Compare each pair of scenarios
            for i, r1 in enumerate(hedging_results):
                for r2 in hedging_results[i + 1:]:
                    if r1.scenario_name != r2.scenario_name:
                        comp = self._compare_results(r1, r2)
                        comparisons.append(comp)

        return comparisons

    def _compare_results(
        self,
        result_a: AggregatedResults,
        result_b: AggregatedResults,
    ) -> StrategyComparison:
        """Compare two results."""
        config_a = f"{result_a.scenario_name} | {result_a.hedging_mode}"
        config_b = f"{result_b.scenario_name} | {result_b.hedging_mode}"

        comparisons = []

        metrics = [
            ("Mean P&L", result_a.mean_pnl, result_b.mean_pnl),
            ("Std Dev", result_a.std_pnl, result_b.std_pnl),
            ("Win Rate", result_a.win_rate * 100, result_b.win_rate * 100),
            ("Sharpe Ratio", result_a.sharpe_ratio, result_b.sharpe_ratio),
            ("Max Gain", result_a.max_pnl, result_b.max_pnl),
            ("Max Loss", result_a.min_pnl, result_b.min_pnl),
            ("5th Percentile", result_a.pnl_5th, result_b.pnl_5th),
            ("95th Percentile", result_a.pnl_95th, result_b.pnl_95th),
        ]

        for metric_name, val_a, val_b in metrics:
            diff = val_b - val_a
            pct_diff = (diff / abs(val_a)) * 100 if val_a != 0 else 0

            comparisons.append(ComparisonResult(
                name_a=config_a,
                name_b=config_b,
                metric=metric_name,
                value_a=val_a,
                value_b=val_b,
                difference=diff,
                pct_difference=pct_diff,
            ))

        return StrategyComparison(
            config_a=config_a,
            config_b=config_b,
            comparisons=comparisons,
        )

    def summary_table(self) -> str:
        """
        Generate a summary table of all results.

        Returns:
            Formatted string with comparison table
        """
        lines = [
            "",
            "=" * 120,
            f"{'Strategy':<25} │ {'Scenario':<20} │ {'Hedging':<10} │ {'Mean P&L':>10} │ {'Std':>8} │ {'Win%':>6} │ {'Sharpe':>7} │ {'5%':>8} │ {'95%':>8}",
            "─" * 120,
        ]

        for r in self.results:
            lines.append(
                f"{r.strategy_name:<25} │ "
                f"{r.scenario_name:<20} │ "
                f"{r.hedging_mode:<10} │ "
                f"${r.mean_pnl:>9.2f} │ "
                f"${r.std_pnl:>7.2f} │ "
                f"{r.win_rate*100:>5.1f}% │ "
                f"{r.sharpe_ratio:>7.2f} │ "
                f"${r.pnl_5th:>7.2f} │ "
                f"${r.pnl_95th:>7.2f}"
            )

        lines.append("=" * 120)
        return "\n".join(lines)

    def best_configuration(self, metric: str = "sharpe_ratio") -> AggregatedResults:
        """
        Find the best configuration by a given metric.

        Args:
            metric: "mean_pnl", "sharpe_ratio", "win_rate", "sortino_ratio"

        Returns:
            Best AggregatedResults
        """
        return max(self.results, key=lambda r: getattr(r, metric))

    def worst_configuration(self, metric: str = "sharpe_ratio") -> AggregatedResults:
        """
        Find the worst configuration by a given metric.

        Args:
            metric: "mean_pnl", "sharpe_ratio", "win_rate", "sortino_ratio"

        Returns:
            Worst AggregatedResults
        """
        return min(self.results, key=lambda r: getattr(r, metric))

    def rank_by_metric(self, metric: str = "sharpe_ratio") -> List[Tuple[int, AggregatedResults]]:
        """
        Rank all configurations by a metric.

        Args:
            metric: Metric to rank by

        Returns:
            List of (rank, result) tuples
        """
        sorted_results = sorted(self.results, key=lambda r: getattr(r, metric), reverse=True)
        return [(i + 1, r) for i, r in enumerate(sorted_results)]

    def get_pnls_by_group(self, group_by: str = "hedging") -> Dict[str, np.ndarray]:
        """
        Get combined P&L arrays grouped by a category.

        Args:
            group_by: "hedging", "scenario", or "strategy"

        Returns:
            Dict mapping group name to combined P&L array
        """
        if group_by == "hedging":
            groups = self.by_hedging
        elif group_by == "scenario":
            groups = self.by_scenario
        elif group_by == "strategy":
            groups = self.by_strategy
        else:
            raise ValueError(f"Unknown group_by: {group_by}")

        result = {}
        for name, results in groups.items():
            combined_pnls = np.concatenate([r.pnls for r in results])
            result[name] = combined_pnls

        return result
