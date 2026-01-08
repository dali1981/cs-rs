#!/usr/bin/env python3
"""
Main simulation runner - Entry point for running option strategy simulations.

This script demonstrates the full capabilities of the modular simulation system:
1. Configure simulations using presets or custom configs
2. Run Monte Carlo simulations
3. Aggregate and analyze results
4. Generate visualizations

Usage:
    # Run with defaults
    $ uv run simulation/run_simulation.py

    # Run with custom parameters
    $ uv run simulation/run_simulation.py --strategy long_call --num-sims 5000 --scenarios all

    # Generate plots
    $ uv run simulation/run_simulation.py --plot --output-dir ./results
"""

import argparse
from pathlib import Path
from typing import Optional
import sys

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent))

from config import (
    quick_config, SimulationConfig, MarketConfig, SimulationParams,
    StrategyPresets, ScenarioPresets, HedgingPresets,
    StrategyConfig, ScenarioConfig, HedgingConfig, HedgingMode,
)
from engine import SimulationEngine, run_quick_simulation
from aggregator import ResultsAggregator
from plotter import SimulationPlotter


def main():
    """Main entry point."""
    parser = argparse.ArgumentParser(
        description="Run option strategy Monte Carlo simulations",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Quick simulation
  uv run simulation/run_simulation.py --strategy long_call --num-sims 1000

  # Compare hedging modes
  uv run simulation/run_simulation.py --strategy long_call --hedging all --scenarios iv_equals_rv

  # Full comparison with plots
  uv run simulation/run_simulation.py --strategy long_call --num-sims 5000 --scenarios all --hedging both --plot

  # Save results
  uv run simulation/run_simulation.py --output-dir ./results --save-plots
        """
    )

    # Strategy options
    parser.add_argument(
        "--strategy", type=str, default="long_call",
        choices=["long_call", "short_call", "long_put", "short_put",
                 "long_straddle", "short_straddle", "bull_call_spread", "iron_condor"],
        help="Strategy to simulate (default: long_call)"
    )

    # Market parameters
    parser.add_argument("--spot", type=float, default=100.0, help="Initial spot price (default: 100)")
    parser.add_argument("--iv", type=float, default=0.25, help="Entry implied volatility (default: 0.25)")
    parser.add_argument("--days", type=int, default=30, help="Days to expiration (default: 30)")

    # Simulation parameters
    parser.add_argument("--num-sims", type=int, default=1000, help="Number of simulations (default: 1000)")
    parser.add_argument("--seed", type=int, default=None, help="Random seed for reproducibility")

    # Scenarios
    parser.add_argument(
        "--scenarios", type=str, default="standard",
        choices=["all", "standard", "iv_equals_rv", "iv_greater_rv", "iv_less_rv"],
        help="Volatility scenarios (default: standard)"
    )

    # Hedging
    parser.add_argument(
        "--hedging", type=str, default="both",
        choices=["none", "daily", "weekly", "both", "all"],
        help="Hedging modes to test (default: both)"
    )

    # Output options
    parser.add_argument("--output-dir", type=Path, default=None, help="Output directory for results")
    parser.add_argument("--plot", action="store_true", help="Show plots")
    parser.add_argument("--save-plots", action="store_true", help="Save plots to output directory")
    parser.add_argument("--quiet", action="store_true", help="Suppress progress output")

    args = parser.parse_args()

    # Build configuration
    config = quick_config(
        strategy=args.strategy,
        spot=args.spot,
        iv=args.iv,
        days=args.days,
        num_sims=args.num_sims,
        scenarios=args.scenarios,
        hedging=args.hedging,
        seed=args.seed,
    )

    # Print configuration
    if not args.quiet:
        print_config(config)

    # Run simulations
    engine = SimulationEngine(progress_bar=not args.quiet)
    results = engine.run(config)

    # Aggregate results
    aggregator = ResultsAggregator(results)

    # Print summary
    if not args.quiet:
        print(aggregator.summary_table())

        # Print detailed stats for best and worst configs
        best = aggregator.best_configuration("sharpe_ratio")
        worst = aggregator.worst_configuration("sharpe_ratio")

        print("\n" + "=" * 70)
        print("BEST CONFIGURATION (by Sharpe Ratio)")
        print("=" * 70)
        print(best.summary())

        print("\n" + "=" * 70)
        print("WORST CONFIGURATION (by Sharpe Ratio)")
        print("=" * 70)
        print(worst.summary())

        # Hedging comparison
        if len(aggregator.hedging_modes) > 1:
            print("\n" + "=" * 70)
            print("HEDGING COMPARISON")
            print("=" * 70)
            for comp in aggregator.compare_hedging():
                print(comp.summary())

    # Create output directory
    if args.output_dir:
        args.output_dir.mkdir(parents=True, exist_ok=True)

        # Save config
        config.save(args.output_dir / "config.json")

        # Save summary
        with open(args.output_dir / "summary.txt", "w") as f:
            f.write(aggregator.summary_table())

    # Plotting
    if args.plot or args.save_plots:
        plotter = SimulationPlotter(results)

        if args.save_plots and args.output_dir:
            saved = plotter.save_all(args.output_dir / "plots")
            if not args.quiet:
                print(f"\nSaved plots to: {args.output_dir / 'plots'}")
                for path in saved:
                    print(f"  - {path.name}")

        if args.plot:
            plotter.plot_all(show=True)

    return results


def print_config(config: SimulationConfig):
    """Print configuration summary."""
    print("\n" + "=" * 70)
    print("SIMULATION CONFIGURATION")
    print("=" * 70)
    print(f"""
Strategy: {config.strategy.name}
  Legs: {config.strategy.num_legs}

Market:
  Spot Price: ${config.market.spot_price:.2f}
  Entry IV: {config.market.entry_iv:.1%}
  Risk-Free Rate: {config.market.risk_free_rate:.1%}

Simulation:
  Days: {config.simulation.num_days}
  Number of Simulations: {config.simulation.num_simulations:,}
  Random Seed: {config.simulation.random_seed or 'None (random)'}

Scenarios: {len(config.scenarios)}
  {', '.join(s.name for s in config.scenarios)}

Hedging Modes: {len(config.hedging_modes)}
  {', '.join(h.mode.value for h in config.hedging_modes)}

Total Runs: {config.total_runs:,}
""")
    print("=" * 70)


# ============================================================================
# EXAMPLE FUNCTIONS - For programmatic use
# ============================================================================

def example_quick_simulation():
    """
    Example: Quick simulation with minimal code.

    This is the simplest way to run a simulation.
    """
    print("\n" + "=" * 70)
    print("Example: Quick Simulation")
    print("=" * 70)

    # One-liner simulation
    results = run_quick_simulation(
        strategy="long_call",
        num_sims=1000,
        scenarios="standard",
        hedging="both",
    )

    # Print results
    aggregator = ResultsAggregator(results)
    print(aggregator.summary_table())

    return results


def example_custom_configuration():
    """
    Example: Custom configuration with full control.

    Use this when you need fine-grained control over parameters.
    """
    print("\n" + "=" * 70)
    print("Example: Custom Configuration")
    print("=" * 70)

    # Custom market
    market = MarketConfig(
        spot_price=150.0,
        entry_iv=0.30,
        risk_free_rate=0.04,
    )

    # Custom strategy
    strategy = StrategyPresets.long_straddle_atm()

    # Custom scenarios
    scenarios = (
        ScenarioPresets.iv_equals_rv(),
        ScenarioPresets.iv_greater_rv(0.70),  # RV is 70% of IV
        ScenarioPresets.iv_less_rv(1.30),     # RV is 130% of IV
    )

    # Custom hedging
    hedging_modes = (
        HedgingPresets.no_hedge(),
        HedgingPresets.daily_hedge(),
        HedgingPresets.threshold_hedge(0.03),  # 3% threshold
    )

    # Build config
    config = SimulationConfig(
        market=market,
        strategy=strategy,
        scenarios=scenarios,
        hedging_modes=hedging_modes,
        simulation=SimulationParams(
            num_simulations=2000,
            num_days=45,
            random_seed=42,
        ),
    )

    # Run
    engine = SimulationEngine()
    results = engine.run(config)

    # Analyze
    aggregator = ResultsAggregator(results)
    print(aggregator.summary_table())

    # Find best config
    best = aggregator.best_configuration("sharpe_ratio")
    print(f"\nBest config: {best.scenario_name} | {best.hedging_mode}")
    print(f"  Sharpe: {best.sharpe_ratio:.2f}")
    print(f"  Mean P&L: ${best.mean_pnl:.2f}")

    return results


def example_strategy_comparison():
    """
    Example: Compare multiple strategies.

    Useful for strategy selection.
    """
    print("\n" + "=" * 70)
    print("Example: Strategy Comparison")
    print("=" * 70)

    strategies = [
        ("long_call", "Long Call"),
        ("short_call", "Short Call"),
        ("long_straddle", "Long Straddle"),
        ("short_straddle", "Short Straddle"),
    ]

    all_results = []

    for strategy_key, strategy_name in strategies:
        results = run_quick_simulation(
            strategy=strategy_key,
            num_sims=1000,
            scenarios="iv_equals_rv",
            hedging="none",
            progress=False,
        )
        all_results.extend(results)

    # Aggregate all
    aggregator = ResultsAggregator(all_results)
    print(aggregator.summary_table())

    # Rank by Sharpe
    print("\nRanking by Sharpe Ratio:")
    for rank, result in aggregator.rank_by_metric("sharpe_ratio"):
        print(f"  {rank}. {result.strategy_name}: {result.sharpe_ratio:.2f}")

    return all_results


def example_with_plotting():
    """
    Example: Full simulation with plots.

    Demonstrates visualization capabilities.
    """
    print("\n" + "=" * 70)
    print("Example: Full Simulation with Plots")
    print("=" * 70)

    # Run simulation
    results = run_quick_simulation(
        strategy="long_call",
        num_sims=2000,
        scenarios="all",
        hedging="both",
        seed=42,
    )

    # Create plotter
    plotter = SimulationPlotter(results)

    # Generate all plots
    plotter.plot_all(show=True)

    return results


if __name__ == "__main__":
    # Check for example flag
    if len(sys.argv) > 1 and sys.argv[1] == "--examples":
        print("\nRunning Examples...")

        print("\n" + "=" * 70)
        print("EXAMPLE 1: Quick Simulation")
        print("=" * 70)
        example_quick_simulation()

        print("\n" + "=" * 70)
        print("EXAMPLE 2: Custom Configuration")
        print("=" * 70)
        example_custom_configuration()

        print("\n" + "=" * 70)
        print("EXAMPLE 3: Strategy Comparison")
        print("=" * 70)
        example_strategy_comparison()

        print("\n" + "=" * 70)
        print("Examples complete!")
        print("Run with --plot flag for visualization examples")
        print("=" * 70)
    else:
        main()
