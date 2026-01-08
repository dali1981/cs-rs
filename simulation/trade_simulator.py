#!/usr/bin/env python3
"""
Main trade simulator - runs comprehensive scenario analysis.

Simulates option strategies across multiple scenarios:
- IV = RV
- IV > RV (vega win)
- IV < RV (vega loss)
- IV increases
- IV crush
"""

import argparse
import json
from pathlib import Path
from typing import List, Dict
import numpy as np
import matplotlib.pyplot as plt
from datetime import datetime

from core_simulator import (
    StockSimulator, GBMConfig, HestonConfig, SimulationModel,
    SCENARIO_IV_EQUALS_RV, SCENARIO_IV_GREATER_RV, SCENARIO_IV_LESS_RV,
    SCENARIO_IV_INCREASES, SCENARIO_IV_CRUSH
)
from strategy_simulator import (
    StrategySimulator, StrategyConfig, OptionLeg, ExitCondition,
    SimulationResult, OptionType
)
from black_scholes import BlackScholes


class TradeSimulator:
    """Comprehensive trade simulation across multiple scenarios."""

    def __init__(
        self,
        spot_price: float = 100.0,
        initial_iv: float = 0.25,
        risk_free_rate: float = 0.05,
        time_to_expiry: float = 30/365,  # 30 days
        simulation_model: SimulationModel = SimulationModel.GBM,
        random_seed: int = None,
    ):
        """
        Initialize simulator.

        Parameters:
        -----------
        spot_price : float
            Current stock price
        initial_iv : float
            Entry implied volatility (also used as drift)
        risk_free_rate : float
            Risk-free rate for pricing
        time_to_expiry : float
            Time to option expiration in years
        simulation_model : SimulationModel
            GBM or Heston
        random_seed : int
            Random seed for reproducibility
        """
        self.spot_price = spot_price
        self.initial_iv = initial_iv
        self.risk_free_rate = risk_free_rate
        self.time_to_expiry = time_to_expiry
        self.simulation_model = simulation_model
        self.random_seed = random_seed

    def create_strategies(self) -> Dict[str, StrategyConfig]:
        """Create a set of test strategies."""
        strategies = {}

        # 1. Long Call
        call_price = BlackScholes.price(
            S=self.spot_price,
            K=self.spot_price,
            T=self.time_to_expiry,
            r=self.risk_free_rate,
            sigma=self.initial_iv,
            option_type=OptionType.CALL,
        )
        strategies["long_call"] = StrategyConfig(
            name="Long Call (ATM)",
            legs=[OptionLeg(OptionType.CALL, self.spot_price, self.time_to_expiry, 1.0)],
            entry_price=call_price,
        )

        # 2. Long Put
        put_price = BlackScholes.price(
            S=self.spot_price,
            K=self.spot_price,
            T=self.time_to_expiry,
            r=self.risk_free_rate,
            sigma=self.initial_iv,
            option_type=OptionType.PUT,
        )
        strategies["long_put"] = StrategyConfig(
            name="Long Put (ATM)",
            legs=[OptionLeg(OptionType.PUT, self.spot_price, self.time_to_expiry, 1.0)],
            entry_price=put_price,
        )

        # 3. Short Call (naked)
        strategies["short_call"] = StrategyConfig(
            name="Short Call (ATM)",
            legs=[OptionLeg(OptionType.CALL, self.spot_price, self.time_to_expiry, -1.0)],
            entry_price=-call_price,
        )

        # 4. Short Put (naked)
        strategies["short_put"] = StrategyConfig(
            name="Short Put (ATM)",
            legs=[OptionLeg(OptionType.PUT, self.spot_price, self.time_to_expiry, -1.0)],
            entry_price=-put_price,
        )

        # 5. Long Strangle (long call + long put, OTM)
        otm_call_strike = self.spot_price * 1.05  # 5% OTM
        otm_put_strike = self.spot_price * 0.95  # 5% OTM

        otm_call_price = BlackScholes.price(
            S=self.spot_price,
            K=otm_call_strike,
            T=self.time_to_expiry,
            r=self.risk_free_rate,
            sigma=self.initial_iv,
            option_type=OptionType.CALL,
        )
        otm_put_price = BlackScholes.price(
            S=self.spot_price,
            K=otm_put_strike,
            T=self.time_to_expiry,
            r=self.risk_free_rate,
            sigma=self.initial_iv,
            option_type=OptionType.PUT,
        )

        strategies["long_strangle"] = StrategyConfig(
            name="Long Strangle (5% OTM)",
            legs=[
                OptionLeg(OptionType.CALL, otm_call_strike, self.time_to_expiry, 1.0),
                OptionLeg(OptionType.PUT, otm_put_strike, self.time_to_expiry, 1.0),
            ],
            entry_price=otm_call_price + otm_put_price,
        )

        # 6. Short Strangle
        strategies["short_strangle"] = StrategyConfig(
            name="Short Strangle (5% OTM)",
            legs=[
                OptionLeg(OptionType.CALL, otm_call_strike, self.time_to_expiry, -1.0),
                OptionLeg(OptionType.PUT, otm_put_strike, self.time_to_expiry, -1.0),
            ],
            entry_price=-(otm_call_price + otm_put_price),
        )

        # 7. Call Spread (bull call spread)
        high_call_strike = self.spot_price * 1.03
        high_call_price = BlackScholes.price(
            S=self.spot_price,
            K=high_call_strike,
            T=self.time_to_expiry,
            r=self.risk_free_rate,
            sigma=self.initial_iv,
            option_type=OptionType.CALL,
        )
        strategies["bull_call_spread"] = StrategyConfig(
            name="Bull Call Spread",
            legs=[
                OptionLeg(OptionType.CALL, self.spot_price, self.time_to_expiry, 1.0),
                OptionLeg(OptionType.CALL, high_call_strike, self.time_to_expiry, -1.0),
            ],
            entry_price=call_price - high_call_price,
        )

        return strategies

    def simulate_all_scenarios(
        self,
        strategy_config: StrategyConfig,
        num_steps: int = 30,
        num_paths: int = 100,
    ) -> Dict[str, SimulationResult]:
        """
        Simulate a strategy across all scenarios.

        Parameters:
        -----------
        strategy_config : StrategyConfig
            Strategy to simulate
        num_steps : int
            Number of simulation steps (days)
        num_paths : int
            Number of Monte Carlo paths

        Returns:
        --------
        Dict[str, SimulationResult]
            Results for each scenario
        """
        # Create stock simulator
        if self.simulation_model == SimulationModel.GBM:
            gbm_config = GBMConfig(
                spot_price=self.spot_price,
                drift_rate=self.initial_iv,  # Use IV as proxy for expected return
                volatility=self.initial_iv,
            )
            path, _ = StockSimulator.simulate_gbm(
                gbm_config,
                self.time_to_expiry,
                num_steps,
                num_paths,
                self.random_seed,
            )
        else:  # HESTON
            heston_config = HestonConfig(
                spot_price=self.spot_price,
                drift_rate=self.initial_iv,
                initial_variance=self.initial_iv ** 2,
                mean_variance=self.initial_iv ** 2,
                variance_of_variance=0.1,  # Vol of vol
                mean_reversion=5.0,  # Speed of reversion
                rho=-0.5,  # Correlation
            )
            path, _ = StockSimulator.simulate_heston(
                heston_config,
                self.time_to_expiry,
                num_steps,
                num_paths,
                self.random_seed,
            )

        # Create simulator
        simulator = StrategySimulator(strategy_config)

        # Run across all scenarios
        scenarios = [
            SCENARIO_IV_EQUALS_RV,
            SCENARIO_IV_GREATER_RV,
            SCENARIO_IV_LESS_RV,
            SCENARIO_IV_INCREASES,
            SCENARIO_IV_CRUSH,
        ]

        results = {}
        for scenario in scenarios:
            result = simulator.simulate(path, scenario)
            results[scenario.name] = result

        return results, path

    def run_full_analysis(
        self,
        output_dir: Path = None,
        num_steps: int = 30,
        num_paths: int = 100,
    ):
        """
        Run complete analysis across all strategies and scenarios.

        Parameters:
        -----------
        output_dir : Path
            Directory to save results and plots
        num_steps : int
            Simulation steps (days)
        num_paths : int
            Monte Carlo paths
        """
        if output_dir is None:
            output_dir = Path.cwd() / "simulation_results"
        output_dir.mkdir(parents=True, exist_ok=True)

        print(f"\n{'='*70}")
        print(f"Trade Simulation - Scenario Analysis")
        print(f"{'='*70}")
        print(f"Spot Price: ${self.spot_price:.2f}")
        print(f"Initial IV: {self.initial_iv:.1%}")
        print(f"Time to Expiry: {self.time_to_expiry:.1%} ({int(self.time_to_expiry * 365)} days)")
        print(f"Model: {self.simulation_model.value.upper()}")
        print(f"Output: {output_dir}")

        # Get strategies
        strategies = self.create_strategies()
        print(f"\nTesting {len(strategies)} strategies...")

        all_results = {}
        summary_data = []

        for strategy_name, strategy_config in strategies.items():
            print(f"\n  Simulating: {strategy_config.name}...", end=" ", flush=True)

            try:
                results, path = self.simulate_all_scenarios(
                    strategy_config,
                    num_steps=num_steps,
                    num_paths=num_paths,
                )
                all_results[strategy_name] = (results, path, strategy_config)

                # Create summary row
                for scenario_name, result in results.items():
                    summary_data.append({
                        "strategy": strategy_config.name,
                        "scenario": scenario_name,
                        "final_pnl": result.final_pnl,
                        "final_pnl_pct": result.final_pnl_pct,
                        "max_loss": result.max_loss,
                        "max_gain": result.max_gain,
                    })

                print("✓")

            except Exception as e:
                print(f"✗ ({str(e)})")
                continue

        # Print summary
        print(f"\n{'='*70}")
        print("Summary Results")
        print(f"{'='*70}\n")

        for strategy_name, (results, path, config) in all_results.items():
            print(f"\n{config.name}")
            print(f"{'-'*70}")
            print(f"{'Scenario':<20} {'Final P&L':>15} {'P&L %':>12} {'Max Loss':>12} {'Max Gain':>12}")
            print(f"{'-'*70}")

            for scenario_name in sorted(results.keys()):
                result = results[scenario_name]
                print(
                    f"{scenario_name:<20} ${result.final_pnl:>14.2f} "
                    f"{result.final_pnl_pct:>11.1f}% "
                    f"${result.max_loss:>11.2f} "
                    f"${result.max_gain:>11.2f}"
                )

        # Save results to JSON
        self._save_results(all_results, output_dir)

        # Generate plots
        self._generate_plots(all_results, output_dir)

        print(f"\n{'='*70}")
        print(f"✓ Analysis complete!")
        print(f"Results saved to: {output_dir}")
        print(f"{'='*70}\n")

    def _save_results(self, all_results, output_dir):
        """Save results to JSON files."""
        for strategy_name, (results, path, config) in all_results.items():
            results_dict = {
                "strategy": config.name,
                "spot_price": self.spot_price,
                "initial_iv": self.initial_iv,
                "time_to_expiry": self.time_to_expiry,
                "scenarios": {}
            }

            for scenario_name, result in results.items():
                results_dict["scenarios"][scenario_name] = {
                    "final_pnl": float(result.final_pnl),
                    "final_pnl_pct": float(result.final_pnl_pct),
                    "max_loss": float(result.max_loss),
                    "max_gain": float(result.max_gain),
                    "realized_volatility": float(result.realized_volatility),
                    "exit_day": result.exit_day,
                    "exit_reason": result.exit_reason,
                }

            output_file = output_dir / f"{strategy_name}_results.json"
            with open(output_file, "w") as f:
                json.dump(results_dict, f, indent=2)

    def _generate_plots(self, all_results, output_dir):
        """Generate visualization plots."""
        for strategy_name, (results, path, config) in all_results.items():
            fig, axes = plt.subplots(3, 2, figsize=(15, 12))
            fig.suptitle(f"{config.name} - Scenario Comparison", fontsize=14, fontweight="bold")

            # Plot 1: Stock Price Paths
            ax = axes[0, 0]
            ax.plot(path.times * 365, path.spot_prices, linewidth=2, label="Stock Price")
            ax.axhline(self.spot_price, color="gray", linestyle="--", alpha=0.5)
            ax.set_xlabel("Days")
            ax.set_ylabel("Price ($)")
            ax.set_title("Stock Price Path")
            ax.grid(True, alpha=0.3)
            ax.legend()

            # Plot 2: P&L by Scenario
            ax = axes[0, 1]
            scenarios = list(results.keys())
            pnls = [results[s].final_pnl for s in scenarios]
            colors = ["green" if p > 0 else "red" for p in pnls]
            ax.bar(range(len(scenarios)), pnls, color=colors, alpha=0.7)
            ax.set_xticks(range(len(scenarios)))
            ax.set_xticklabels(scenarios, rotation=45, ha="right")
            ax.set_ylabel("Final P&L ($)")
            ax.set_title("Final P&L by Scenario")
            ax.axhline(0, color="black", linestyle="-", linewidth=0.5)
            ax.grid(True, alpha=0.3, axis="y")

            # Plot 3: P&L over time (IV = RV scenario)
            ax = axes[1, 0]
            result_equal = results[SCENARIO_IV_EQUALS_RV.name]
            pnls_over_time = [state.position_pnl for state in result_equal.daily_states]
            days = [state.day for state in result_equal.daily_states]
            ax.plot(days, pnls_over_time, linewidth=2, label="P&L")
            ax.fill_between(days, 0, pnls_over_time, alpha=0.3)
            ax.set_xlabel("Days")
            ax.set_ylabel("P&L ($)")
            ax.set_title(f"P&L Evolution ({SCENARIO_IV_EQUALS_RV.name})")
            ax.axhline(0, color="black", linestyle="-", linewidth=0.5)
            ax.grid(True, alpha=0.3)
            ax.legend()

            # Plot 4: Delta over time
            ax = axes[1, 1]
            deltas_equal = [state.delta for state in result_equal.daily_states]
            ax.plot(days, deltas_equal, linewidth=2, label="Delta")
            ax.set_xlabel("Days")
            ax.set_ylabel("Delta")
            ax.set_title(f"Delta Evolution ({SCENARIO_IV_EQUALS_RV.name})")
            ax.axhline(0, color="black", linestyle="-", linewidth=0.5)
            ax.grid(True, alpha=0.3)
            ax.legend()

            # Plot 5: Max Loss and Max Gain
            ax = axes[2, 0]
            max_losses = [results[s].max_loss for s in scenarios]
            max_gains = [results[s].max_gain for s in scenarios]
            x = np.arange(len(scenarios))
            width = 0.35
            ax.bar(x - width/2, max_losses, width, label="Max Loss", color="red", alpha=0.7)
            ax.bar(x + width/2, max_gains, width, label="Max Gain", color="green", alpha=0.7)
            ax.set_xticks(x)
            ax.set_xticklabels(scenarios, rotation=45, ha="right")
            ax.set_ylabel("P&L ($)")
            ax.set_title("Max Loss and Gain by Scenario")
            ax.axhline(0, color="black", linestyle="-", linewidth=0.5)
            ax.grid(True, alpha=0.3, axis="y")
            ax.legend()

            # Plot 6: P&L components (IV=RV scenario)
            ax = axes[2, 1]
            theta_pnl = [state.pnl_theta for state in result_equal.daily_states]
            gamma_pnl = [state.pnl_gamma for state in result_equal.daily_states]
            vega_pnl = [state.pnl_vega for state in result_equal.daily_states]
            ax.plot(days, np.cumsum(theta_pnl), label="Theta", linewidth=2)
            ax.plot(days, np.cumsum(gamma_pnl), label="Gamma", linewidth=2)
            ax.plot(days, np.cumsum(vega_pnl), label="Vega", linewidth=2)
            ax.set_xlabel("Days")
            ax.set_ylabel("Cumulative P&L ($)")
            ax.set_title(f"P&L Components ({SCENARIO_IV_EQUALS_RV.name})")
            ax.grid(True, alpha=0.3)
            ax.legend()

            plt.tight_layout()
            output_file = output_dir / f"{strategy_name}_analysis.png"
            plt.savefig(output_file, dpi=150, bbox_inches="tight")
            plt.close()


def main():
    parser = argparse.ArgumentParser(
        description="Trade simulation with scenario analysis",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Basic simulation with default parameters
  uv run python3 trade_simulator.py

  # Custom spot price and IV
  uv run python3 trade_simulator.py --spot 105 --iv 0.30

  # Use Heston model instead of GBM
  uv run python3 trade_simulator.py --model heston

  # Specify output directory
  uv run python3 trade_simulator.py --output ~/my_results

  # Custom time to expiry (45 days)
  uv run python3 trade_simulator.py --dte 45
        """
    )

    parser.add_argument("--spot", type=float, default=100.0,
                       help="Initial spot price (default: 100)")
    parser.add_argument("--iv", type=float, default=0.25,
                       help="Initial implied volatility (default: 0.25)")
    parser.add_argument("--dte", type=int, default=30,
                       help="Days to expiration (default: 30)")
    parser.add_argument("--rate", type=float, default=0.05,
                       help="Risk-free rate (default: 0.05)")
    parser.add_argument("--model", choices=["gbm", "heston"], default="gbm",
                       help="Simulation model (default: gbm)")
    parser.add_argument("--steps", type=int, default=30,
                       help="Simulation steps/days (default: 30)")
    parser.add_argument("--paths", type=int, default=100,
                       help="Monte Carlo paths (default: 100)")
    parser.add_argument("--seed", type=int, default=42,
                       help="Random seed (default: 42)")
    parser.add_argument("--output", type=Path, default=None,
                       help="Output directory (default: ./simulation_results)")

    args = parser.parse_args()

    # Create simulator
    simulator = TradeSimulator(
        spot_price=args.spot,
        initial_iv=args.iv,
        risk_free_rate=args.rate,
        time_to_expiry=args.dte / 365,
        simulation_model=SimulationModel.GBM if args.model == "gbm" else SimulationModel.HESTON,
        random_seed=args.seed,
    )

    # Run analysis
    simulator.run_full_analysis(
        output_dir=args.output,
        num_steps=args.steps,
        num_paths=args.paths,
    )


if __name__ == "__main__":
    main()
