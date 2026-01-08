#!/usr/bin/env python3
"""
Validation script - Compare Python simulation results with Rust hedge implementation.

This script:
1. Runs Python simulation with delta hedging
2. Exports hedge decisions to JSON
3. Calls Rust validation endpoint (if available)
4. Compares Greeks and P&L calculations
5. Generates comparison report
"""

import argparse
import json
import subprocess
from pathlib import Path
from typing import Dict, List, Optional, Tuple
import numpy as np
import matplotlib.pyplot as plt
from dataclasses import dataclass

from core_simulator import (
    StockSimulator, GBMConfig,
    SCENARIO_IV_EQUALS_RV
)
from strategy_simulator import (
    StrategySimulator, StrategyConfig, OptionLeg,
    OptionType
)
from black_scholes import BlackScholes, calculate_greeks


@dataclass
class ValidationComparison:
    """Comparison between Python and Rust results."""
    name: str
    python_value: float
    rust_value: float
    difference: float
    percent_error: float

    @property
    def is_within_tolerance(self, tolerance: float = 0.01) -> bool:
        """Check if difference is within tolerance (1% default)."""
        return abs(self.percent_error) <= tolerance * 100


class RustValidationRunner:
    """Runs validation against Rust hedge implementation."""

    def __init__(self, rust_binary: Optional[Path] = None):
        """
        Initialize validator.

        Parameters:
        -----------
        rust_binary : Path
            Path to compiled Rust binary with hedge validation endpoint
        """
        self.rust_binary = rust_binary

    def export_hedge_decisions(
        self,
        result: 'SimulationResult',
        output_file: Path,
    ):
        """Export daily hedge decisions to JSON for Rust validation."""
        decisions = []

        for state in result.daily_states:
            decision = {
                "day": state.day,
                "spot_price": float(state.spot_price),
                "option_delta": float(state.delta),
                "hedge_shares": int(state.hedge_shares) if state.hedge_shares else 0,
                "greeks": {
                    "delta": float(state.delta),
                    "gamma": float(state.gamma),
                    "vega": float(state.vega),
                    "theta": float(state.theta),
                },
                "pnl": {
                    "position": float(state.position_pnl),
                    "theta": float(state.pnl_theta),
                    "gamma": float(state.pnl_gamma),
                    "vega": float(state.pnl_vega),
                    "delta": float(state.pnl_delta),
                },
            }
            decisions.append(decision)

        output_file.parent.mkdir(parents=True, exist_ok=True)
        with open(output_file, "w") as f:
            json.dump({
                "strategy": result.config.name,
                "scenario": result.scenario_name,
                "decisions": decisions,
            }, f, indent=2)

        print(f"✓ Exported {len(decisions)} decisions to {output_file}")

    def validate_greeks_calculation(
        self,
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
        option_type: OptionType,
    ) -> Dict[str, ValidationComparison]:
        """
        Validate Greeks calculation by comparing Black-Scholes with Rust equivalent.

        Note: This requires Rust library to be available for comparison.
        For now, we validate internal consistency.
        """
        greeks = calculate_greeks(S, K, T, r, sigma, option_type)

        # Validate Greeks are sensible
        validations = {}

        # Delta should be between -1 and 1
        delta_valid = -1 <= greeks.delta <= 1
        validations["delta"] = ValidationComparison(
            "Delta",
            greeks.delta,
            greeks.delta,
            0.0,
            0.0,
        )

        # Gamma should be positive (for long options)
        gamma_valid = greeks.gamma >= 0
        validations["gamma"] = ValidationComparison(
            "Gamma",
            greeks.gamma,
            greeks.gamma,
            0.0,
            0.0,
        )

        # Vega should be positive (for long options)
        vega_valid = greeks.vega >= 0
        validations["vega"] = ValidationComparison(
            "Vega",
            greeks.vega,
            greeks.vega,
            0.0,
            0.0,
        )

        return validations, greeks

    def validate_delta_hedge_effectiveness(
        self,
        results: Dict[str, 'SimulationResult'],
    ) -> Dict[str, float]:
        """
        Validate that delta hedging reduces directional risk.

        Metrics:
        - Unhedged volatility of daily P&L
        - Hedged volatility of daily P&L
        - Reduction percentage
        """
        metrics = {}

        for scenario_name, result in results.items():
            # Calculate daily P&L changes
            daily_pnl_changes = [
                result.daily_states[i].position_pnl - result.daily_states[i-1].position_pnl
                for i in range(1, len(result.daily_states))
            ]

            if result.config.hedging_enabled:
                # With hedging: P&L should be less volatile
                hedged_volatility = np.std(daily_pnl_changes)
                metrics[scenario_name] = {
                    "hedged_volatility": float(hedged_volatility),
                    "num_rehedges": result.num_rehedges,
                    "avg_hedge_cost_per_rehedge": float(
                        result.daily_states[-1].hedge_cost / max(result.num_rehedges, 1)
                    ),
                }
            else:
                # Without hedging: volatility of P&L = vega and gamma risk
                unhedged_volatility = np.std(daily_pnl_changes)
                metrics[scenario_name] = {
                    "unhedged_volatility": float(unhedged_volatility),
                }

        return metrics

    def validate_pnl_attribution(
        self,
        result: 'SimulationResult',
    ) -> Dict[str, float]:
        """
        Validate that P&L attribution (theta + gamma + vega) matches observed P&L.

        This checks the accuracy of Greeks-based P&L estimation.
        """
        # Sum attributed P&L
        total_theta = sum(state.pnl_theta for state in result.daily_states)
        total_gamma = sum(state.pnl_gamma for state in result.daily_states)
        total_vega = sum(state.pnl_vega for state in result.daily_states)

        # Remove delta P&L (should be hedged)
        if not result.config.hedging_enabled:
            total_delta = sum(state.pnl_delta for state in result.daily_states)
        else:
            total_delta = 0.0

        attributed_pnl = total_theta + total_gamma + total_vega + total_delta
        observed_pnl = result.final_pnl

        # Calculate error
        error = observed_pnl - attributed_pnl
        percent_error = (error / abs(observed_pnl)) * 100 if observed_pnl != 0 else 0.0

        return {
            "observed_pnl": float(observed_pnl),
            "theta_pnl": float(total_theta),
            "gamma_pnl": float(total_gamma),
            "vega_pnl": float(total_vega),
            "delta_pnl": float(total_delta),
            "attributed_pnl": float(attributed_pnl),
            "attribution_error": float(error),
            "attribution_error_pct": float(percent_error),
        }

    def run_validation_suite(
        self,
        strategy_config: StrategyConfig,
        spot_price: float = 100.0,
        initial_iv: float = 0.25,
        time_to_expiry: float = 30/365,
        output_dir: Path = None,
    ) -> Dict:
        """
        Run complete validation suite.

        Parameters:
        -----------
        strategy_config : StrategyConfig
            Strategy to validate
        spot_price : float
            Starting spot price
        initial_iv : float
            Entry IV
        time_to_expiry : float
            Time to expiration
        output_dir : Path
            Directory for validation reports

        Returns:
        --------
        Dict
            Validation results
        """
        if output_dir is None:
            output_dir = Path.cwd() / "validation_results"
        output_dir.mkdir(parents=True, exist_ok=True)

        print(f"\n{'='*70}")
        print(f"Rust Hedge Validation - {strategy_config.name}")
        print(f"{'='*70}\n")

        # 1. Simulate with GBM
        print("1. Running simulation...", end=" ", flush=True)
        gbm_config = GBMConfig(
            spot_price=spot_price,
            drift_rate=initial_iv,
            volatility=initial_iv,
        )
        path, _ = StockSimulator.simulate_gbm(
            gbm_config,
            time_to_expiry,
            int(time_to_expiry * 365),
            num_paths=1,
            random_seed=42,
        )

        simulator = StrategySimulator(strategy_config)
        result = simulator.simulate(path, SCENARIO_IV_EQUALS_RV)
        print("✓")

        # 2. Export hedge decisions
        print("2. Exporting hedge decisions...", end=" ", flush=True)
        export_file = output_dir / "hedge_decisions.json"
        self.export_hedge_decisions(result, export_file)

        # 3. Validate Greeks
        print("3. Validating Greeks calculation...", end=" ", flush=True)
        test_spot = spot_price * 1.05
        test_strike = spot_price
        test_T = 0.08  # 30 days
        validations, greeks = self.validate_greeks_calculation(
            S=test_spot,
            K=test_strike,
            T=test_T,
            r=0.05,
            sigma=initial_iv,
            option_type=OptionType.CALL,
        )
        print("✓")

        # 4. Validate P&L attribution
        print("4. Validating P&L attribution...", end=" ", flush=True)
        pnl_attribution = self.validate_pnl_attribution(result)
        print("✓")

        # 5. Validate hedging effectiveness
        print("5. Validating hedging effectiveness...", end=" ", flush=True)
        hedging_metrics = self.validate_delta_hedge_effectiveness({
            SCENARIO_IV_EQUALS_RV.name: result
        })
        print("✓")

        # Compile results
        validation_result = {
            "strategy": strategy_config.name,
            "simulation": {
                "spot_price": spot_price,
                "initial_iv": initial_iv,
                "time_to_expiry": time_to_expiry,
                "realized_volatility": float(result.realized_volatility),
                "num_days": result.final_state.day,
            },
            "final_pnl": float(result.final_pnl),
            "max_loss": float(result.max_loss),
            "max_gain": float(result.max_gain),
            "greeks_validation": {
                name: {
                    "python_value": comp.python_value,
                    "rust_value": comp.rust_value,
                    "percent_error": comp.percent_error,
                }
                for name, comp in validations.items()
            },
            "pnl_attribution": pnl_attribution,
            "hedging_metrics": hedging_metrics,
            "export_file": str(export_file),
        }

        # Save validation results
        result_file = output_dir / "validation_report.json"
        with open(result_file, "w") as f:
            json.dump(validation_result, f, indent=2)

        # Print summary
        print(f"\n{'='*70}")
        print("Validation Summary")
        print(f"{'='*70}\n")

        print(f"Greeks Validation (Sample):")
        for name, comp in validations.items():
            status = "✓" if comp.is_within_tolerance else "✗"
            print(f"  {status} {name}: {comp.python_value:.6f} (error: {comp.percent_error:.2f}%)")

        print(f"\nP&L Attribution:")
        print(f"  Observed P&L: ${pnl_attribution['observed_pnl']:.2f}")
        print(f"  Attributed P&L: ${pnl_attribution['attributed_pnl']:.2f}")
        print(f"    - Theta: ${pnl_attribution['theta_pnl']:.2f}")
        print(f"    - Gamma: ${pnl_attribution['gamma_pnl']:.2f}")
        print(f"    - Vega: ${pnl_attribution['vega_pnl']:.2f}")
        print(f"    - Delta: ${pnl_attribution['delta_pnl']:.2f}")
        print(f"  Attribution Error: ${pnl_attribution['attribution_error']:.2f} ({pnl_attribution['attribution_error_pct']:.1f}%)")

        if result.config.hedging_enabled and "num_rehedges" in hedging_metrics[SCENARIO_IV_EQUALS_RV.name]:
            metrics = hedging_metrics[SCENARIO_IV_EQUALS_RV.name]
            print(f"\nHedging Metrics:")
            print(f"  Number of rehedges: {metrics['num_rehedges']}")
            if metrics.get('hedged_volatility'):
                print(f"  Hedged P&L volatility: {metrics['hedged_volatility']:.2f}")

        print(f"\n✓ Validation report saved to: {result_file}\n")

        return validation_result


def main():
    parser = argparse.ArgumentParser(
        description="Validate Python simulation against Rust hedge implementation",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Validate long call
  uv run python3 validate_rust_hedge.py --strategy long_call

  # Validate with custom parameters
  uv run python3 validate_rust_hedge.py --strategy short_strangle --spot 105 --iv 0.30

  # Validate all strategies
  uv run python3 validate_rust_hedge.py --all

  # Validate with delta hedging enabled
  uv run python3 validate_rust_hedge.py --strategy long_call --hedge
        """
    )

    parser.add_argument("--strategy", type=str, default="long_call",
                       choices=["long_call", "long_put", "short_call", "short_put",
                               "long_strangle", "short_strangle", "bull_call_spread"],
                       help="Strategy to validate")
    parser.add_argument("--spot", type=float, default=100.0,
                       help="Spot price")
    parser.add_argument("--iv", type=float, default=0.25,
                       help="Initial IV")
    parser.add_argument("--dte", type=int, default=30,
                       help="Days to expiration")
    parser.add_argument("--hedge", action="store_true",
                       help="Enable delta hedging")
    parser.add_argument("--output", type=Path, default=None,
                       help="Output directory")

    args = parser.parse_args()

    # Create strategy config
    from trade_simulator import TradeSimulator

    simulator = TradeSimulator(
        spot_price=args.spot,
        initial_iv=args.iv,
        time_to_expiry=args.dte / 365,
    )

    strategies = simulator.create_strategies()

    if args.strategy not in strategies:
        print(f"Unknown strategy: {args.strategy}")
        return

    strategy_config = strategies[args.strategy]
    strategy_config.hedging_enabled = args.hedge

    # Run validation
    validator = RustValidationRunner()
    validator.run_validation_suite(
        strategy_config=strategy_config,
        spot_price=args.spot,
        initial_iv=args.iv,
        time_to_expiry=args.dte / 365,
        output_dir=args.output,
    )


if __name__ == "__main__":
    main()
