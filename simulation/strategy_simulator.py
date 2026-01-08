#!/usr/bin/env python3
"""
Generic option strategy simulator with delta hedging support.

Allows simulation of any option strategy:
- Single legs (long/short call/put)
- Multi-leg strategies (strangle, butterfly, iron condor, etc.)
- With or without delta hedging
- Various exit conditions (hold to expiry, take profit, stop loss, time target)
"""

import numpy as np
from dataclasses import dataclass, field
from typing import List, Dict, Optional, Callable
from enum import Enum
from datetime import datetime, timedelta

from black_scholes import (
    BlackScholes, OptionType, calculate_greeks, GreeksSummary
)
from core_simulator import SimulationPath, VolatilityScenario


class ExitCondition(Enum):
    """When to exit the trade."""
    HOLD_TO_EXPIRY = "hold"
    PROFIT_TARGET = "take_profit"
    STOP_LOSS = "stop_loss"
    TIME_TARGET = "time_target"


@dataclass
class OptionLeg:
    """Single option leg in a strategy."""
    option_type: OptionType  # Call or Put
    strike: float
    expiration: float  # Time to expiry in years (from trade entry)
    position_size: float = 1.0  # 1.0 = long 1, -1.0 = short 1
    quantity: int = 1  # Number of contracts (100 shares per contract in practice)

    def __post_init__(self):
        if self.expiration <= 0:
            raise ValueError("expiration must be positive")
        if self.quantity <= 0:
            raise ValueError("quantity must be positive")


@dataclass
class StrategyConfig:
    """Configuration for a trading strategy."""
    name: str
    legs: List[OptionLeg]
    entry_price: float  # Debit/credit paid at entry
    risk_free_rate: float = 0.05  # 5% annual rate
    hedging_enabled: bool = False
    hedging_frequency: int = 1  # Rehedge every N days
    hedging_threshold: float = 0.05  # Rehedge if delta drift > this
    exit_condition: ExitCondition = ExitCondition.HOLD_TO_EXPIRY
    exit_param: Optional[float] = None  # Target profit/loss or days
    max_loss: Optional[float] = None  # Maximum loss threshold


@dataclass
class DailyState:
    """State of the position on a single day."""
    day: int
    time: float  # Time elapsed in years
    spot_price: float
    implied_volatility: float  # Current IV in scenario
    time_to_expiry: float  # Remaining time

    # Greeks
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    # Position value
    position_price: float = 0.0  # Current value of entire position
    position_pnl: float = 0.0  # Unrealized P&L from entry
    position_pnl_pct: float = 0.0

    # Hedging info
    hedge_shares: Optional[int] = None  # Shares held for delta hedge
    hedge_cost: float = 0.0  # Cumulative hedging cost
    hedge_pnl: float = 0.0  # P&L from hedging

    # Detailed P&L breakdown
    pnl_theta: float = 0.0  # From time decay
    pnl_gamma: float = 0.0  # From gamma
    pnl_vega: float = 0.0  # From volatility change
    pnl_delta: float = 0.0  # From directional move (unhedged)

    # Exit decision
    should_exit: bool = False
    exit_reason: Optional[str] = None


@dataclass
class SimulationResult:
    """Complete simulation result."""
    config: StrategyConfig
    scenario_name: str
    daily_states: List[DailyState]
    final_pnl: float
    final_pnl_pct: float
    max_loss: float
    max_gain: float
    realized_volatility: float
    implied_volatility_realized: float  # Actual IV path realized
    num_rehedges: int = 0
    exit_day: Optional[int] = None
    exit_reason: Optional[str] = None

    @property
    def final_state(self) -> DailyState:
        return self.daily_states[-1]

    @property
    def initial_state(self) -> DailyState:
        return self.daily_states[0]


class StrategySimulator:
    """Simulates an option strategy over a stock price path."""

    def __init__(self, config: StrategyConfig):
        """Initialize simulator with strategy configuration."""
        self.config = config

    def simulate(
        self,
        path: SimulationPath,
        scenario: VolatilityScenario,
    ) -> SimulationResult:
        """
        Simulate the strategy over a stock price path.

        Parameters:
        -----------
        path : SimulationPath
            Simulated stock price path
        scenario : VolatilityScenario
            IV scenario to apply

        Returns:
        --------
        SimulationResult
            Complete simulation with daily states and P&L breakdown
        """
        daily_states = []
        initial_spot = path.initial_price
        hedge_shares = 0 if self.config.hedging_enabled else None

        # Calculate entry Greeks
        entry_greeks = self._calculate_position_greeks(
            spot=initial_spot,
            time=0.0,
            time_remaining=max(leg.expiration for leg in self.config.legs),
            iv=scenario.get_iv_at_step(path.realized_volatility, 0, len(path.spot_prices)),
        )

        cumulative_hedge_cost = 0.0
        cumulative_hedge_pnl = 0.0
        last_hedge_day = 0
        days_since_hedge = 0

        # Simulate each day
        for day in range(len(path.spot_prices)):
            time_elapsed = path.times[day]
            spot_price = path.spot_prices[day]
            time_remaining = max(leg.expiration for leg in self.config.legs) - time_elapsed

            # Handle expired legs
            if time_remaining <= 0:
                time_remaining = 0

            # Get IV for this step
            iv = scenario.get_iv_at_step(path.realized_volatility, day, len(path.spot_prices))

            # Calculate Greeks
            position_greeks = self._calculate_position_greeks(
                spot=spot_price,
                time=time_elapsed,
                time_remaining=time_remaining,
                iv=iv,
            )

            # Price position
            position_price = self._calculate_position_price(
                spot=spot_price,
                time=time_elapsed,
                time_remaining=time_remaining,
                iv=iv,
            )

            # P&L calculations
            unrealized_pnl = position_price - self.config.entry_price
            unrealized_pnl_pct = (unrealized_pnl / abs(self.config.entry_price)) * 100 if self.config.entry_price != 0 else 0

            # Detailed P&L breakdown (approximation using Greeks)
            if day > 0:
                prev_state = daily_states[-1]
                spot_move = spot_price - prev_state.spot_price
                iv_move = iv - prev_state.implied_volatility
                time_decay = path.times[day] - path.times[day - 1]

                pnl_theta = prev_state.theta * time_decay  # Time decay per day
                pnl_delta = prev_state.delta * spot_move  # Delta P&L
                pnl_gamma = 0.5 * prev_state.gamma * (spot_move ** 2)  # Gamma P&L
                pnl_vega = prev_state.vega * iv_move  # Vega P&L
            else:
                pnl_theta = 0.0
                pnl_delta = 0.0
                pnl_gamma = 0.0
                pnl_vega = 0.0

            # Handle hedging
            if self.config.hedging_enabled and day > 0:
                days_since_hedge += 1

                # Check if should rehedge
                delta_drift = abs(position_greeks.delta - (hedge_shares / initial_spot if hedge_shares else 0))

                if (days_since_hedge >= self.config.hedging_frequency or
                    delta_drift > self.config.hedging_threshold):

                    # Rehedge
                    target_hedge = int(round(-position_greeks.delta * initial_spot))
                    hedge_adjustment = target_hedge - hedge_shares
                    hedge_cost_this_adjustment = -hedge_adjustment * spot_price  # Negative = cost of buying

                    cumulative_hedge_cost += hedge_cost_this_adjustment
                    cumulative_hedge_pnl += hedge_shares * (spot_price - prev_state.spot_price if day > 0 else 0)

                    hedge_shares = target_hedge
                    days_since_hedge = 0
                    last_hedge_day = day
            elif self.config.hedging_enabled and day == 0:
                # Initial hedge
                target_hedge = int(round(-entry_greeks.delta * initial_spot))
                hedge_shares = target_hedge
                # No cost at entry (already included in entry_price)

            # Check exit conditions
            should_exit, exit_reason = self._check_exit_conditions(
                day=day,
                unrealized_pnl=unrealized_pnl,
                unrealized_pnl_pct=unrealized_pnl_pct,
                time_remaining=time_remaining,
                total_days=len(path.spot_prices),
            )

            # Create daily state
            state = DailyState(
                day=day,
                time=time_elapsed,
                spot_price=spot_price,
                implied_volatility=iv,
                time_to_expiry=time_remaining,
                delta=position_greeks.delta,
                gamma=position_greeks.gamma,
                vega=position_greeks.vega,
                theta=position_greeks.theta,
                position_price=position_price,
                position_pnl=unrealized_pnl,
                position_pnl_pct=unrealized_pnl_pct,
                hedge_shares=hedge_shares,
                hedge_cost=cumulative_hedge_cost,
                hedge_pnl=cumulative_hedge_pnl,
                pnl_theta=pnl_theta,
                pnl_gamma=pnl_gamma,
                pnl_vega=pnl_vega,
                pnl_delta=pnl_delta,
                should_exit=should_exit,
                exit_reason=exit_reason,
            )

            daily_states.append(state)

            if should_exit:
                break

        # Calculate final metrics
        final_state = daily_states[-1]
        final_pnl = final_state.position_pnl
        final_pnl_pct = final_state.position_pnl_pct

        # Calculate max loss and max gain
        max_loss = min(state.position_pnl for state in daily_states)
        max_gain = max(state.position_pnl for state in daily_states)

        # Count rehedges
        num_rehedges = sum(1 for state in daily_states[1:] if state.hedge_shares != daily_states[daily_states.index(state) - 1].hedge_shares)

        return SimulationResult(
            config=self.config,
            scenario_name=scenario.name,
            daily_states=daily_states,
            final_pnl=final_pnl,
            final_pnl_pct=final_pnl_pct,
            max_loss=max_loss,
            max_gain=max_gain,
            realized_volatility=path.realized_volatility,
            implied_volatility_realized=np.mean([
                scenario.get_iv_at_step(path.realized_volatility, i, len(path.spot_prices))
                for i in range(len(path.spot_prices))
            ]),
            num_rehedges=num_rehedges,
            exit_day=final_state.day if final_state.should_exit else None,
            exit_reason=final_state.exit_reason,
        )

    def _calculate_position_price(
        self,
        spot: float,
        time: float,
        time_remaining: float,
        iv: float,
    ) -> float:
        """Calculate total position price."""
        total_price = 0.0

        for leg in self.config.legs:
            # Time to leg expiration
            leg_time_remaining = leg.expiration - time
            if leg_time_remaining <= 0:
                # Leg expired - intrinsic value only
                if leg.option_type == OptionType.CALL:
                    leg_price = max(spot - leg.strike, 0)
                else:
                    leg_price = max(leg.strike - spot, 0)
            else:
                leg_price = BlackScholes.price(
                    S=spot,
                    K=leg.strike,
                    T=leg_time_remaining,
                    r=self.config.risk_free_rate,
                    sigma=iv,
                    option_type=leg.option_type,
                )

            total_price += leg_price * leg.position_size * leg.quantity

        return total_price

    def _calculate_position_greeks(
        self,
        spot: float,
        time: float,
        time_remaining: float,
        iv: float,
    ) -> GreeksSummary:
        """Calculate total position Greeks."""
        total_delta = 0.0
        total_gamma = 0.0
        total_vega = 0.0
        total_theta = 0.0
        total_rho = 0.0

        for leg in self.config.legs:
            # Time to leg expiration
            leg_time_remaining = leg.expiration - time
            if leg_time_remaining <= 0:
                # Expired leg has no Greeks
                continue

            leg_greeks = calculate_greeks(
                S=spot,
                K=leg.strike,
                T=leg_time_remaining,
                r=self.config.risk_free_rate,
                sigma=iv,
                option_type=leg.option_type,
            )

            # Apply position size and quantity
            multiplier = leg.position_size * leg.quantity

            total_delta += leg_greeks.delta * multiplier
            total_gamma += leg_greeks.gamma * multiplier
            total_vega += leg_greeks.vega * multiplier
            total_theta += leg_greeks.theta * multiplier
            total_rho += leg_greeks.rho * multiplier

        return GreeksSummary(
            price=self._calculate_position_price(spot, time, time_remaining, iv),
            delta=total_delta,
            gamma=total_gamma,
            vega=total_vega,
            theta=total_theta,
            rho=total_rho,
        )

    def _check_exit_conditions(
        self,
        day: int,
        unrealized_pnl: float,
        unrealized_pnl_pct: float,
        time_remaining: float,
        total_days: int,
    ) -> tuple:
        """Check if position should be exited."""
        if self.config.exit_condition == ExitCondition.HOLD_TO_EXPIRY:
            # Exit only if expired
            return time_remaining <= 0, "Expiry" if time_remaining <= 0 else None

        elif self.config.exit_condition == ExitCondition.PROFIT_TARGET:
            if unrealized_pnl >= self.config.exit_param:
                return True, f"Profit target reached (${unrealized_pnl:.2f})"

        elif self.config.exit_condition == ExitCondition.STOP_LOSS:
            if unrealized_pnl <= -self.config.exit_param:
                return True, f"Stop loss hit (-${abs(unrealized_pnl):.2f})"

        elif self.config.exit_condition == ExitCondition.TIME_TARGET:
            days_elapsed = day
            if days_elapsed >= self.config.exit_param:
                return True, f"Time target reached ({days_elapsed} days)"

        # Check absolute max loss
        if self.config.max_loss is not None and unrealized_pnl <= -self.config.max_loss:
            return True, f"Max loss exceeded (-${abs(unrealized_pnl):.2f})"

        # Check expiry
        if time_remaining <= 0:
            return True, "Expiry"

        return False, None
