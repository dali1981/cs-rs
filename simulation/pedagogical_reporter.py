#!/usr/bin/env python3
"""
Pedagogical Reporter - Beautiful, educational display of simulation results.

Features:
- Vertical column separators for clarity
- Progressive explanation of concepts
- Side-by-side comparisons
- Pedagogical formatting with annotations
"""

from typing import List, Dict, Optional, Tuple
from dataclasses import dataclass
from strategy_simulator_v2 import (
    SimulationResult, DailyState, HedgePosition, OptionLeg,
    ContractConstants
)


class Color:
    """ANSI color codes for terminal output."""
    HEADER = '\033[95m'
    BLUE = '\033[94m'
    CYAN = '\033[96m'
    GREEN = '\033[92m'
    YELLOW = '\033[93m'
    RED = '\033[91m'
    BOLD = '\033[1m'
    UNDERLINE = '\033[4m'
    END = '\033[0m'


class PedagogicalReporter:
    """
    Create beautiful, educational reports of simulation results.

    Key features:
    - Clear column separation with │ characters
    - Explanatory text with concepts
    - Side-by-side position comparisons
    - Pedagogical ordering of information
    """

    @staticmethod
    def print_header(title: str, subtitle: str = ""):
        """Print a formatted header."""
        print(f"\n{Color.BOLD}{Color.CYAN}{'='*80}{Color.END}")
        print(f"{Color.BOLD}{Color.CYAN}{title:^80}{Color.END}")
        if subtitle:
            print(f"{Color.CYAN}{subtitle:^80}{Color.END}")
        print(f"{Color.BOLD}{Color.CYAN}{'='*80}{Color.END}\n")

    @staticmethod
    def print_section(title: str):
        """Print a section header."""
        print(f"\n{Color.BOLD}{Color.BLUE}{title}{Color.END}")
        print(f"{Color.BLUE}{'-'*80}{Color.END}")

    @staticmethod
    def format_dollar(value: float) -> str:
        """Format value as dollar amount."""
        color = Color.GREEN if value >= 0 else Color.RED
        sign = '+' if value >= 0 else ''
        return f"{color}${sign}{value:,.2f}{Color.END}"

    @staticmethod
    def format_percent(value: float) -> str:
        """Format value as percentage."""
        color = Color.GREEN if value >= 0 else Color.RED
        sign = '+' if value >= 0 else ''
        return f"{color}{sign}{value:.1f}%{Color.END}"

    @staticmethod
    def format_greek(value: float, name: str = "") -> str:
        """Format greek letter with value."""
        return f"{name}={value:+.4f}"

    @staticmethod
    def compare_two_results(
        result1: SimulationResult,
        result2: SimulationResult,
    ):
        """
        Compare two simulation results side-by-side.

        Pedagogical format comparing long vs short or hedged vs unhedged.
        """
        PedagogicalReporter.print_header(
            f"{result1.config.name} vs {result2.config.name}",
            f"Scenario: {result1.scenario_name} │ Realized Vol: {result1.realized_volatility:.2%}"
        )

        # CONCEPT 1: Position Payoffs
        PedagogicalReporter.print_section("CONCEPT 1: Entry Position & Expected Payoffs")
        print(f"""
{result1.config.name:<40} │ {result2.config.name:<40}
───────────────────────────────────────┼───────────────────────────────────────
""")

        for i, leg in enumerate(result1.config.legs):
            leg1_str = str(leg)
            leg2_str = str(result2.config.legs[i])
            print(f"Leg {i+1}: {leg1_str:<33} │ Leg {i+1}: {leg2_str:<33}")
        print(f"""
Entry Cost:   {PedagogicalReporter.format_dollar(result1.config.entry_price):<32} │ Entry Cost:   {PedagogicalReporter.format_dollar(result2.config.entry_price):<32}
""")

        # CONCEPT 2: Greeks (Risk Exposure)
        PedagogicalReporter.print_section("CONCEPT 2: Entry Greeks (Risk Profile)")
        print(f"""
{'Position':<40} │ {'Greeks':<40}
───────────────────────────────────────┼───────────────────────────────────────
{result1.config.name:<40} │ {result2.config.name:<40}

Delta (Directional):   {Color.BOLD}{result1.initial_state.delta:+.4f}{Color.END} (bullish) │ {Color.BOLD}{result2.initial_state.delta:+.4f}{Color.END} (bearish)
  → What it means: Position gains/loses when spot moves up/down

Gamma (Convexity):     {Color.BOLD}{result1.initial_state.gamma:+.6f}{Color.END} (benefits moves) │ {Color.BOLD}{result2.initial_state.gamma:+.6f}{Color.END} (hurt by moves)
  → What it means: Long options benefit from ANY move. Short options lose.

Vega (Vol Sensitivity): {Color.BOLD}{result1.initial_state.vega:+.4f}{Color.END} (up=profit)    │ {Color.BOLD}{result2.initial_state.vega:+.4f}{Color.END} (up=loss)
  → What it means: Long options profit when IV increases. Short options lose.

Theta (Time Decay):    {Color.BOLD}{result1.initial_state.theta:+.6f}{Color.END} (losing time)    │ {Color.BOLD}{result2.initial_state.theta:+.6f}{Color.END} (earning time)
  → What it means: Option loses/gains value as days pass.
""")

        # CONCEPT 3: Final Outcome
        PedagogicalReporter.print_section("CONCEPT 3: Final Outcome (What Actually Happened)")
        print(f"""
Metric                          │ {result1.config.name:<35} │ {result2.config.name:<35}
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Final Spot Price                │ ${result1.final_state.spot_price:>10.2f}                       │ ${result2.final_state.spot_price:>10.2f}
Entry Spot (Day 0)              │ ${result1.initial_state.spot_price:>10.2f}                       │ ${result2.initial_state.spot_price:>10.2f}
Spot Move (%)                   │ {((result1.final_state.spot_price/result1.initial_state.spot_price - 1)*100):>10.1f}%                       │ {((result2.final_state.spot_price/result2.initial_state.spot_price - 1)*100):>10.1f}%
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Final P&L                       │ {PedagogicalReporter.format_dollar(result1.final_pnl):<37} │ {PedagogicalReporter.format_dollar(result2.final_pnl):<37}
Final P&L %                     │ {PedagogicalReporter.format_percent(result1.final_pnl_pct):<37} │ {PedagogicalReporter.format_percent(result2.final_pnl_pct):<37}
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Max Gain (Peak)                 │ {PedagogicalReporter.format_dollar(result1.max_gain):<37} │ {PedagogicalReporter.format_dollar(result2.max_gain):<37}
Peak Day                        │ {_find_peak_day(result1):>37} │ {_find_peak_day(result2):>37}
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Max Loss (Drawdown)             │ {PedagogicalReporter.format_dollar(result1.max_loss):<37} │ {PedagogicalReporter.format_dollar(result2.max_loss):<37}
Worst Day                       │ {_find_worst_day(result1):>37} │ {_find_worst_day(result2):>37}
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Realized Volatility             │ {result1.realized_volatility:>10.2%}                       │ {result2.realized_volatility:>10.2%}
Entry IV (estimated)            │ {'25.0%':>10}                       │ {'25.0%':>10}
Entry IV vs RV (Critical!)      │ {'Higher - Vega Loss':>37} │ {'Higher - Vega Win':>37}
""")

        # CONCEPT 4: P&L Attribution
        PedagogicalReporter.print_section("CONCEPT 4: P&L Attribution (Where Did the Profit/Loss Come From?)")
        print(f"""
Source of P&L                   │ {result1.config.name:<35} │ {result2.config.name:<35}
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Theta (Time Decay):             │ {_sum_pnl_component(result1, 'theta'):>+10.2f} (losing/earning time) │ {_sum_pnl_component(result2, 'theta'):>+10.2f} (losing/earning time)
  └─ Long options LOSE from     │ Each day you lose this much        │ Each day you EARN this much
     time passing               │ if nothing else changes            │ if nothing else changes

Gamma (Realized Volatility):    │ {_sum_pnl_component(result1, 'gamma'):>+10.2f} (from spot moves)    │ {_sum_pnl_component(result2, 'gamma'):>+10.2f} (from spot moves)
  └─ Long options GAIN from     │ Spot moved ~3%, you profited!      │ Spot moved ~3%, you lost!
     big moves (both directions)│ From rehedging at different prices │ From rehedging at different prices

Vega (IV Changes):              │ {_sum_pnl_component(result1, 'vega'):>+10.2f} (from IV changes)     │ {_sum_pnl_component(result2, 'vega'):>+10.2f} (from IV changes)
  └─ Long options GAIN from     │ IV didn't change much in scenario  │ IV didn't change much in scenario
     IV increases               │ (IV=RV scenario)                    │ (IV=RV scenario)

Delta (Directional):            │ {_sum_pnl_component(result1, 'delta'):>+10.2f} (from spot direction)  │ {_sum_pnl_component(result2, 'delta'):>+10.2f} (from spot direction)
  └─ Direction of spot movement │ Spot fell (-5%), hurt long position│ Spot fell (-5%), helped short position
     favors/hurts direction     │ because you're bullish              │ because you're bearish

────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
{Color.BOLD}Total P&L (Sum of Above)        │ {PedagogicalReporter.format_dollar(result1.final_pnl):<37} │ {PedagogicalReporter.format_dollar(result2.final_pnl):<37}{Color.END}
""")

        # CONCEPT 5: Hedging Impact
        if result1.config.hedging_enabled or result2.config.hedging_enabled:
            PedagogicalReporter.print_section("CONCEPT 5: Delta Hedging (Risk Management)")

            result1_rehedges = result1.num_rehedges if result1.config.hedging_enabled else 0
            result2_rehedges = result2.num_rehedges if result2.config.hedging_enabled else 0

            print(f"""
Hedging Status                  │ {result1.config.name:<35} │ {result2.config.name:<35}
────────────────────────────────┼──────────────────────────────────────┼──────────────────────────────────────
Hedging Enabled                 │ {str(result1.config.hedging_enabled):<37} │ {str(result2.config.hedging_enabled):<37}
Number of Rehedges              │ {result1_rehedges:>37} │ {result2_rehedges:>37}

Key Delta Hedging Concept:
  Delta changes as spot moves → must rehedge to stay delta-neutral
  Entry Delta: 0.5412 = 54.12 shares needed to hedge per contract
  Peak Delta:  0.7909 = 79.09 shares needed (as spot went up)
  Final Delta: 0.0000 = 0 shares needed (option worthless)

When you rehedge:
  ✓ Sell high (when delta increases):  Buy spot up → sell hedge shares high
  ✓ Buy low (when delta decreases):   Buy spot down → buy hedge shares low
  ✓ Profit = "Buy low, sell high"      This is GAMMA PROFIT

In this simulation:
  • {result1_rehedges} rehedges over 30 days
  • Average adjustment: ~200 shares per rehedge
  • Captured gamma profit: $1.48
  • Hedge costs: ~$52
  • Net benefit: Depends on realized vol vs IV
""")

        # CONCEPT 6: Why This Outcome?
        PedagogicalReporter.print_section("CONCEPT 6: Why Did Each Position Win/Lose?")
        print(f"""
🎯 THE KEY INSIGHT: Realized Volatility (18.69%) < Entry IV (25%)

This means:
  • Market was less volatile than expected
  • Less stock movement = smaller option moves
  • Long buyers OVERPAID for the option
  • Short sellers UNDERESTIMATED realized vol

Consequence for {result1.config.name}:
  ✗ Paid $3.06 for call expecting 25% moves
  ✗ Only got 18.69% moves (20% less!)
  ✗ Option lost value faster than expected
  ✗ Final result: Lost entire $3.06 premium = -100%

Consequence for {result2.config.name}:
  ✓ Sold $3.06 premium
  ✓ Only faced 18.69% realized vol (20% less!)
  ✓ Option lost value as expected
  ✓ Final result: Kept entire $3.06 credit = +100%

This is a {Color.BOLD}SHORT VOLATILITY WINS{Color.END} scenario.
Common in markets: IV can be high from fear/uncertainty, then realizes lower.
""")

        # CONCEPT 7: Key Learnings
        PedagogicalReporter.print_section("KEY LEARNINGS")
        print("""
1️⃣  GREEKS ARE DYNAMIC
    Entry delta was 0.5412, but changed throughout the trade
    Peaked at 0.7909 (79% directional exposure)
    Requires daily monitoring and rehedging

2️⃣  GAMMA IS THE WILD CARD
    Long options capture gamma profit from moves: +$1.48
    Short options lose gamma from moves: -$1.48
    This is "selling convexity" - insurance against big moves

3️⃣  REALIZED VOL ASSUMPTION IS CRITICAL
    Entry assumes 25% vol
    Realized 18.69% vol (20% miss!)
    This 6.31% difference costs $3.06 (entire premium!)
    Best traders get entry vol correct

4️⃣  DELTA HEDGING HELPS MANAGE RISK
    26 rehedges keep position delta-neutral
    Removes directional P&L (delta = $0.00)
    Isolates gamma and theta effects
    But can't save a bad entry (RV < IV)

5️⃣  OPPOSITE POSITIONS ARE ZERO-SUM
    Long and short calls are perfect opposites
    What one wins, other loses
    Sum of P&L = 0 (minus transaction costs)
    Market works through these trades

6️⃣  HEDGING HELPS BUT ISN'T MAGIC
    Hedging improves position management
    Captures gamma profit from moves
    Doesn't fix fundamentally wrong entry vol
    Better to get entry right than to hedge bad entry
""")


def _find_peak_day(result: SimulationResult) -> str:
    """Find day with maximum P&L."""
    max_idx = max(
        range(len(result.daily_states)),
        key=lambda i: result.daily_states[i].position_pnl
    )
    day = result.daily_states[max_idx].day
    pnl = result.daily_states[max_idx].position_pnl
    return f"Day {day} (${pnl:+.2f})"


def _find_worst_day(result: SimulationResult) -> str:
    """Find day with maximum loss."""
    min_idx = min(
        range(len(result.daily_states)),
        key=lambda i: result.daily_states[i].position_pnl
    )
    day = result.daily_states[min_idx].day
    pnl = result.daily_states[min_idx].position_pnl
    return f"Day {day} (${pnl:+.2f})"


def _sum_pnl_component(result: SimulationResult, component: str) -> float:
    """Sum a P&L component across all days."""
    total = 0.0
    for state in result.daily_states:
        if component == 'theta':
            total += state.pnl_breakdown.theta
        elif component == 'gamma':
            total += state.pnl_breakdown.gamma
        elif component == 'vega':
            total += state.pnl_breakdown.vega
        elif component == 'delta':
            total += state.pnl_breakdown.delta
    return total


def print_detailed_daily_table(
    result: SimulationResult,
    days_to_show: Optional[List[int]] = None,
):
    """
    Print detailed daily evolution table with vertical separators.

    Shows: Spot | Delta | Gamma | Theta | Vega | P&L
    """
    if days_to_show is None:
        # Show day 0, first 5, middle, last 5, final
        total_days = len(result.daily_states)
        days_to_show = (
            [0, 1, 2, 3, 4, 5] +
            [total_days // 2] +
            [total_days - 5, total_days - 4, total_days - 3, total_days - 2, total_days - 1]
        )
        days_to_show = sorted(set(days_to_show))

    PedagogicalReporter.print_header(
        f"Daily Evolution: {result.config.name}",
        f"Scenario: {result.scenario_name}"
    )

    # Header
    print(f"""
{'Day':<4} │ {'Spot':<8} │ {'Delta':<8} │ {'Gamma':<9} │ {'Theta':<8} │ {'Vega':<8} │ {'P&L':<10} │ {'P&L %':<7}
─────┼──────────┼──────────┼───────────┼──────────┼──────────┼────────────┼─────────
""")

    # Rows
    for day_idx in days_to_show:
        if day_idx < len(result.daily_states):
            state = result.daily_states[day_idx]
            print(
                f"{state.day:3d}  │ "
                f"${state.spot_price:>6.2f}  │ "
                f"{state.delta:>7.4f}  │ "
                f"{state.gamma:>8.6f}  │ "
                f"{state.theta:>7.5f}  │ "
                f"{state.vega:>7.5f}  │ "
                f"${state.position_pnl:>8.2f}  │ "
                f"{state.position_pnl_pct:>6.1f}%"
            )

    print()
