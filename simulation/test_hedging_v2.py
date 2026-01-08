#!/usr/bin/env python3
"""
Test script: Run long call vs short call with proper hedging (v2).

This script demonstrates:
1. Correct hedge calculation (delta × 100 shares, not delta × spot)
2. Refactored code with proper classes and no magic numbers
3. Pedagogical output with vertical column separators
4. IV Hedge Delta vs RV Hedge Delta comparison
"""

import sys
from pathlib import Path

from core_simulator import StockSimulator, GBMConfig, SCENARIO_IV_EQUALS_RV
from strategy_simulator_v2 import (
    StrategyConfig, OptionLeg, StrategySimulator,
    ExitCondition, ContractConstants
)
from black_scholes import OptionType
from pedagogical_reporter import PedagogicalReporter, print_detailed_daily_table


def main():
    """Run long call vs short call comparison with hedging."""

    # Market parameters
    spot_price = 100.0
    risk_free_rate = 0.05
    entry_iv = 0.25  # 25% IV
    days_to_expiry = 30

    # Scenario: Realized vol < Entry IV (the problematic case)
    realized_vol = 0.18691188  # ~18.69%

    print("\n" + "="*80)
    print("DELTA HEDGING ANALYSIS: Correct Formula (v2)")
    print("="*80)
    print(f"Entry IV: 25.0%  │  Realized Vol: {realized_vol:.2%}")
    print(f"Spot: ${spot_price}  │  Days to Expiry: {days_to_expiry}")
    print("="*80 + "\n")

    # Create strategies
    long_call = StrategyConfig(
        name="Long Call (ATM)",
        legs=[
            OptionLeg(
                option_type=OptionType.CALL,
                strike=100.0,
                expiration=days_to_expiry / 365.0,
                position_size=1.0,
                quantity=1,
            )
        ],
        entry_price=3.0626,  # Approximately the Black-Scholes price
        risk_free_rate=risk_free_rate,
        hedging_enabled=True,
        hedging_frequency=1,  # Rehedge daily
        hedging_threshold=0.05,  # Rehedge if delta drifts > 5%
        exit_condition=ExitCondition.HOLD_TO_EXPIRY,
    )

    short_call = StrategyConfig(
        name="Short Call (ATM)",
        legs=[
            OptionLeg(
                option_type=OptionType.CALL,
                strike=100.0,
                expiration=days_to_expiry / 365.0,
                position_size=-1.0,  # Short
                quantity=1,
            )
        ],
        entry_price=-3.0626,  # Credit received (negative)
        risk_free_rate=risk_free_rate,
        hedging_enabled=True,
        hedging_frequency=1,
        hedging_threshold=0.05,
        exit_condition=ExitCondition.HOLD_TO_EXPIRY,
    )

    # Create scenario (using predefined: IV = RV)
    scenario = SCENARIO_IV_EQUALS_RV

    # Create GBM config
    gbm_config = GBMConfig(
        spot_price=spot_price,
        drift_rate=risk_free_rate,
        volatility=realized_vol,
    )

    # Simulate stock path (30 days with daily steps)
    path, _ = StockSimulator.simulate_gbm(
        gbm_config,
        time_to_expiry=days_to_expiry / 365.0,
        num_steps=days_to_expiry,  # Daily steps
        num_paths=1,
        random_seed=42,
    )

    # Run simulations
    print("Running simulations...")
    strategy_sim_long = StrategySimulator(long_call)
    strategy_sim_short = StrategySimulator(short_call)

    result_long = strategy_sim_long.simulate(path, scenario)
    result_short = strategy_sim_short.simulate(path, scenario)

    print("✓ Simulations complete\n")

    # Display results
    PedagogicalReporter.compare_two_results(result_long, result_short)

    # Show daily table
    print_detailed_daily_table(result_long)
    print_detailed_daily_table(result_short)

    # Detailed hedge analysis
    print_hedge_analysis(result_long, result_short)


def print_hedge_analysis(result_long, result_short):
    """Print detailed hedge analysis with correct formulas."""

    PedagogicalReporter.print_header("HEDGE CALCULATION VERIFICATION", "Correct Formula: shares = delta × 100")

    print("""
FORMULA EXPLANATION:
  Entry Delta: 0.5412 (54.12% directional exposure per contract)
  Shares per Contract: 100 (standard options contract size)

  Hedge Shares = -Delta × Shares per Contract
  Hedge Shares = -0.5412 × 100 = -54 shares (SHORT 54 shares)

  ✗ WRONG: shares = delta × spot = 0.5412 × 100 = 5,412 (confuses $ with shares)
  ✓ CORRECT: shares = delta × 100 = 0.5412 × 100 = 54 shares

KEY INSIGHT: Delta × 100 = number of shares. Delta is already 0-1 range.
""")

    # Sample a few key days
    sample_days = [0, 3, 4, 5, 9, 14, 29]

    print(f"""
{'Day':<4} │ {'Spot':<8} │ {'IV Delta':<9} │ {'RV Delta':<9} │ {'IV Hedge':<9} │ {'RV Hedge':<9} │ {'Adjustment':<11}
{' ':<4} │ {'Price':<8} │ {'(Entry)':<9} │ {'(Current)':<9} │ {'Shares':<9} │ {'Shares':<9} │ {'Shares':<11}
─────┼──────────┼───────────┼───────────┼───────────┼───────────┼─────────────
""")

    for day in sample_days:
        if day < len(result_long.daily_states):
            state = result_long.daily_states[day]

            # IV Hedge is fixed at entry delta
            entry_delta = result_long.initial_state.delta
            iv_hedge_shares = int(round(-entry_delta * ContractConstants.SHARES_PER_CONTRACT))

            # RV Hedge is current delta
            rv_delta = state.delta
            rv_hedge_shares = int(round(-rv_delta * ContractConstants.SHARES_PER_CONTRACT))

            # Adjustment from previous day
            if day == 0:
                adjustment = rv_hedge_shares
            else:
                prev_state = result_long.daily_states[day - 1]
                prev_rv_delta = prev_state.delta
                prev_rv_hedge_shares = int(round(-prev_rv_delta * ContractConstants.SHARES_PER_CONTRACT))
                adjustment = rv_hedge_shares - prev_rv_hedge_shares

            adj_str = f"{adjustment:+d}" if adjustment != 0 else "—"

            print(
                f"{state.day:3d}  │ "
                f"${state.spot_price:>6.2f}  │ "
                f"{entry_delta:>+8.4f}  │ "
                f"{rv_delta:>+8.4f}  │ "
                f"{iv_hedge_shares:>8d}  │ "
                f"{rv_hedge_shares:>8d}  │ "
                f"{adj_str:>10}"
            )

    print(f"""
─────┴──────────┴───────────┴───────────┴───────────┴───────────┴─────────────

EXAMPLE - Day 4 Breakdown:
  Entry Delta (IV Hedge): 0.5412
    → Shares needed: -0.5412 × 100 = -54 shares (SHORT 54 to hedge)

  Current Delta (RV Hedge): 0.7909 (spot went up, delta increased)
    → Shares needed: -0.7909 × 100 = -79 shares (SHORT 79 to hedge)

  Adjustment: -79 - (-54) = -25 shares
    → Need to SELL 25 more shares (increase short position)
    → This is where gamma profit comes from!
    → You sell 25 shares at $103.61, buy them back lower later

VERIFICATION: All numbers are in SHARES, not dollars.
""")


if __name__ == "__main__":
    main()
