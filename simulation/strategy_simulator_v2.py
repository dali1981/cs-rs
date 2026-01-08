#!/usr/bin/env python3
"""
Generic option strategy simulator with delta hedging support (v2 - Refactored).

Properly encapsulated with:
- Named constants instead of magic numbers
- Type-safe classes for all components
- Clear hedge calculation (shares = delta × contract_size)
- Pedagogical output

Allows simulation of any option strategy:
- Single legs (long/short call/put)
- Multi-leg strategies (strangle, butterfly, iron condor, etc.)
- With or without delta hedging
- Various exit conditions (hold to expiry, take profit, stop loss, time target)
"""

import numpy as np
from dataclasses import dataclass, field
from typing import List, Dict, Optional
from enum import Enum
from abc import ABC, abstractmethod

from black_scholes import (
    BlackScholes, OptionType, calculate_greeks, GreeksSummary
)
from core_simulator import SimulationPath, VolatilityScenario


# ============================================================================
# CONSTANTS - No Magic Numbers
# ============================================================================

class ContractConstants:
    """Standard options contract parameters."""
    SHARES_PER_CONTRACT = 100  # 1 option contract = 100 shares
    TRADING_DAYS_PER_YEAR = 252


class HedgingConstants:
    """Delta hedging parameters."""
    DEFAULT_REHEDGE_FREQUENCY = 1  # Days between rehedges
    DEFAULT_DELTA_DRIFT_THRESHOLD = 0.05  # 5% delta drift triggers rehedge
    DEFAULT_ROUNDING_SHARES = 1  # Round to nearest 1 share


# ============================================================================
# ENUMS
# ============================================================================

class ExitCondition(Enum):
    """When to exit the trade."""
    HOLD_TO_EXPIRY = "hold"
    PROFIT_TARGET = "take_profit"
    STOP_LOSS = "stop_loss"
    TIME_TARGET = "time_target"


# ============================================================================
# CLASSES FOR OPTION COMPONENTS
# ============================================================================

@dataclass
class OptionLeg:
    """
    Single option leg in a strategy.

    Example:
        leg = OptionLeg(
            option_type=OptionType.CALL,
            strike=100.0,
            expiration=30/365,  # 30 days
            position_size=1.0,  # long 1 contract
            quantity=1,
        )
    """
    option_type: OptionType  # Call or Put
    strike: float  # Strike price in dollars
    expiration: float  # Time to expiry in years (from trade entry)
    position_size: float = 1.0  # 1.0 = long, -1.0 = short
    quantity: int = 1  # Number of contracts

    def __post_init__(self):
        if self.expiration <= 0:
            raise ValueError("expiration must be positive years")
        if self.quantity <= 0:
            raise ValueError("quantity must be positive")
        if not -1 <= self.position_size <= 1:
            raise ValueError("position_size must be -1.0 to +1.0")

    @property
    def shares_represented(self) -> int:
        """Total shares represented by this leg."""
        return self.quantity * ContractConstants.SHARES_PER_CONTRACT

    def __str__(self) -> str:
        direction = "LONG" if self.position_size > 0 else "SHORT"
        return f"{direction} {self.quantity}x {self.option_type.name} ${self.strike:.2f}"


# ============================================================================
# CLASSES FOR HEDGING
# ============================================================================

@dataclass
class HedgePosition:
    """
    Encapsulates a delta hedge position.

    A hedge is shares sold short to offset option delta.

    Example:
        hedge = HedgePosition(
            option_delta=0.5412,  # Option has 54.12% directional exposure
            shares_to_hold=-54,   # Need to short 54 shares
            spot_price=100.0,
        )
        # When spot goes up $1:
        #   Option gains: +0.5412 × $100 = +$54.12
        #   Short position loses: -54 × $1 = -$54
        #   Net: ~$0 (delta-neutral)
    """
    option_delta: float  # Current option delta (directional sensitivity)
    shares_to_hold: int  # Shares shorted for hedging (negative = short)
    spot_price: float  # Current spot price

    @classmethod
    def from_delta(cls, option_delta: float, spot_price: float) -> "HedgePosition":
        """
        Create hedge position from delta.

        Formula: shares_to_hold = -option_delta × shares_per_contract

        Args:
            option_delta: Option delta (0.54 means 54% directional exposure)
            spot_price: Current spot price (for reference only)

        Returns:
            HedgePosition with calculated shares

        Example:
            >>> hedge = HedgePosition.from_delta(0.5412, 100.0)
            >>> hedge.shares_to_hold
            -54  # Short 54 shares to offset 0.5412 × 100 share exposure
        """
        shares = int(round(-option_delta * ContractConstants.SHARES_PER_CONTRACT))
        return cls(
            option_delta=option_delta,
            shares_to_hold=shares,
            spot_price=spot_price,
        )

    @property
    def delta_exposure(self) -> float:
        """
        Current delta exposure as percentage.

        Example:
            >>> hedge = HedgePosition.from_delta(0.5412, 100.0)
            >>> hedge.delta_exposure
            0.54
        """
        return self.shares_to_hold / ContractConstants.SHARES_PER_CONTRACT

    def adjustment_needed(self, new_delta: float) -> int:
        """
        Calculate shares to buy/sell to rehedge to new delta.

        Args:
            new_delta: New option delta after market moves

        Returns:
            Shares to trade (positive = buy, negative = sell)

        Example:
            >>> hedge = HedgePosition.from_delta(0.54, 100.0)
            >>> hedge.adjustment_needed(0.79)  # Delta increased
            -25  # Need to sell 25 more shares
        """
        new_hedge = HedgePosition.from_delta(new_delta, self.spot_price)
        adjustment = new_hedge.shares_to_hold - self.shares_to_hold
        return adjustment

    def __str__(self) -> str:
        direction = "SHORT" if self.shares_to_hold < 0 else "LONG"
        return f"{direction} {abs(self.shares_to_hold)} shares (delta: {self.delta_exposure:+.2%})"


# ============================================================================
# CLASSES FOR STRATEGY CONFIGURATION
# ============================================================================

@dataclass
class StrategyConfig:
    """
    Configuration for a trading strategy.

    Example:
        config = StrategyConfig(
            name="Long Call (ATM)",
            legs=[
                OptionLeg(
                    option_type=OptionType.CALL,
                    strike=100,
                    expiration=30/365,  # 30 days
                    position_size=1.0,
                )
            ],
            entry_price=3.50,  # What you paid (debit)
            hedging_enabled=True,
            hedging_frequency=1,  # Rehedge daily
            hedging_threshold=0.05,  # Rehedge if delta drifts > 5%
        )
    """
    name: str
    legs: List[OptionLeg]
    entry_price: float  # Debit for long, negative for short (credit received)
    risk_free_rate: float = 0.05  # 5% annual rate

    # Hedging parameters
    hedging_enabled: bool = False
    hedging_frequency: int = HedgingConstants.DEFAULT_REHEDGE_FREQUENCY
    hedging_threshold: float = HedgingConstants.DEFAULT_DELTA_DRIFT_THRESHOLD

    # Exit parameters
    exit_condition: ExitCondition = ExitCondition.HOLD_TO_EXPIRY
    exit_param: Optional[float] = None  # Target profit/loss or days
    max_loss: Optional[float] = None  # Maximum loss threshold

    def __post_init__(self):
        if not self.legs:
            raise ValueError("Must have at least one leg")
        if self.hedging_frequency < 1:
            raise ValueError("hedging_frequency must be >= 1")
        if not 0 < self.hedging_threshold < 1:
            raise ValueError("hedging_threshold must be between 0 and 1")

    @property
    def total_legs(self) -> int:
        """Total number of option legs."""
        return len(self.legs)

    @property
    def is_spread(self) -> bool:
        """Whether this is a spread (multiple legs)."""
        return self.total_legs > 1

    def __str__(self) -> str:
        return f"{self.name} ({self.total_legs} legs, ${self.entry_price:.2f})"


# ============================================================================
# CLASSES FOR DAILY STATE
# ============================================================================

@dataclass
class PnLBreakdown:
    """
    Break down of P&L into Greeks components.

    P&L ≈ theta + gamma + vega + delta
    """
    theta: float  # Time decay component
    gamma: float  # Realized volatility / convexity component
    vega: float  # IV change component
    delta: float  # Directional movement component (usually 0 if hedged)

    @property
    def total(self) -> float:
        """Sum of all components."""
        return self.theta + self.gamma + self.vega + self.delta

    def __str__(self) -> str:
        return (
            f"Θ={self.theta:+.2f} "
            f"Γ={self.gamma:+.2f} "
            f"ν={self.vega:+.2f} "
            f"Δ={self.delta:+.2f}"
        )


@dataclass
class DailyState:
    """
    State of the position on a single day.

    Tracks market conditions, Greeks, P&L, and hedging.
    """
    day: int  # Day number (0 to N)
    time: float  # Time elapsed in years
    spot_price: float  # Stock price today
    implied_volatility: float  # IV in current scenario
    time_to_expiry: float  # Remaining time to expiration

    # Greeks
    delta: float = 0.0
    gamma: float = 0.0
    vega: float = 0.0
    theta: float = 0.0

    # Position value and P&L
    position_price: float = 0.0  # Current value of all option legs
    position_pnl: float = 0.0  # Unrealized P&L
    position_pnl_pct: float = 0.0  # P&L as percentage

    # Hedging
    hedge_position: Optional[HedgePosition] = None
    cumulative_hedge_cost: float = 0.0  # Total cost of rehedging
    cumulative_hedge_pnl: float = 0.0  # P&L from hedge trades

    # P&L breakdown
    pnl_breakdown: PnLBreakdown = field(default_factory=lambda: PnLBreakdown(0, 0, 0, 0))

    # Exit decision
    should_exit: bool = False
    exit_reason: Optional[str] = None

    def __str__(self) -> str:
        return (
            f"Day {self.day:2d} | "
            f"Spot ${self.spot_price:7.2f} | "
            f"Δ={self.delta:+.4f} | "
            f"P&L ${self.position_pnl:+7.2f} ({self.position_pnl_pct:+6.1f}%) | "
            f"{self.pnl_breakdown}"
        )


# ============================================================================
# CLASSES FOR SIMULATION RESULTS
# ============================================================================

@dataclass
class SimulationResult:
    """Complete result of strategy simulation."""
    config: StrategyConfig
    scenario_name: str
    daily_states: List[DailyState]
    final_pnl: float
    final_pnl_pct: float
    max_loss: float
    max_gain: float
    realized_volatility: float
    implied_volatility_realized: float
    num_rehedges: int = 0
    exit_day: Optional[int] = None
    exit_reason: Optional[str] = None

    @property
    def final_state(self) -> DailyState:
        return self.daily_states[-1]

    @property
    def initial_state(self) -> DailyState:
        return self.daily_states[0]

    @property
    def duration_days(self) -> int:
        """How many days the position lasted."""
        return self.final_state.day


# ============================================================================
# MAIN SIMULATOR CLASS
# ============================================================================

class StrategySimulator:
    """
    Simulates an option strategy over a stock price path.

    Handles:
    - Multi-leg option strategies
    - Black-Scholes pricing and Greeks
    - Delta hedging with configurable frequency
    - P&L attribution
    - Various exit conditions

    Example:
        config = StrategyConfig(
            name="Long Call",
            legs=[OptionLeg(OptionType.CALL, 100, 30/365)],
            entry_price=3.50,
            hedging_enabled=True,
        )
        simulator = StrategySimulator(config)
        result = simulator.simulate(path, scenario)
    """

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

        Args:
            path: Simulated stock price path
            scenario: Volatility scenario (IV evolution)

        Returns:
            SimulationResult with daily states and metrics
        """
        daily_states = []
        initial_spot = path.initial_price

        # Calculate entry Greeks
        entry_greeks = self._calculate_position_greeks(
            spot=initial_spot,
            time=0.0,
            time_remaining=max(leg.expiration for leg in self.config.legs),
            iv=scenario.get_iv_at_step(path.realized_volatility, 0, len(path.spot_prices)),
        )

        # Initialize hedge if enabled
        hedge_position = None
        cumulative_hedge_cost = 0.0
        cumulative_hedge_pnl = 0.0
        days_since_hedge = 0
        prev_spot = initial_spot

        if self.config.hedging_enabled:
            hedge_position = HedgePosition.from_delta(entry_greeks.delta, initial_spot)

        # Simulate each day
        for day in range(len(path.spot_prices)):
            time_elapsed = path.times[day]
            spot_price = path.spot_prices[day]
            time_remaining = max(leg.expiration for leg in self.config.legs) - time_elapsed
            time_remaining = max(time_remaining, 0)

            # Get IV for this step
            iv = scenario.get_iv_at_step(
                path.realized_volatility, day, len(path.spot_prices)
            )

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

            # P&L
            unrealized_pnl = position_price - self.config.entry_price
            unrealized_pnl_pct = (
                (unrealized_pnl / abs(self.config.entry_price)) * 100
                if self.config.entry_price != 0 else 0
            )

            # P&L breakdown (approximation using Greeks)
            pnl_breakdown = self._calculate_pnl_breakdown(
                day, daily_states, position_greeks, spot_price, iv, path
            )

            # Handle hedging
            if self.config.hedging_enabled and day > 0 and hedge_position:
                days_since_hedge += 1

                # Check if should rehedge
                delta_drift = abs(
                    position_greeks.delta - hedge_position.delta_exposure
                )

                should_rehedge = (
                    days_since_hedge >= self.config.hedging_frequency or
                    delta_drift > self.config.hedging_threshold
                )

                if should_rehedge:
                    # Calculate rehedge adjustment
                    adjustment = hedge_position.adjustment_needed(
                        position_greeks.delta
                    )

                    # Cost of adjustment
                    hedge_adjustment_cost = -adjustment * spot_price

                    # P&L from existing hedge
                    prev_state = daily_states[-1]
                    if prev_state.hedge_position:
                        hedge_pnl_today = (
                            prev_state.hedge_position.shares_to_hold *
                            (spot_price - prev_state.spot_price)
                        )
                        cumulative_hedge_pnl += hedge_pnl_today

                    cumulative_hedge_cost += hedge_adjustment_cost

                    # Update hedge
                    hedge_position = HedgePosition.from_delta(
                        position_greeks.delta, spot_price
                    )

                    days_since_hedge = 0

            elif self.config.hedging_enabled and day == 0:
                # Initial hedge already set
                pass

            # Check exit conditions
            should_exit, exit_reason = self._check_exit_conditions(
                day, unrealized_pnl, unrealized_pnl_pct,
                time_remaining, len(path.spot_prices)
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
                hedge_position=hedge_position,
                cumulative_hedge_cost=cumulative_hedge_cost,
                cumulative_hedge_pnl=cumulative_hedge_pnl,
                pnl_breakdown=pnl_breakdown,
                should_exit=should_exit,
                exit_reason=exit_reason,
            )

            daily_states.append(state)
            prev_spot = spot_price

            if should_exit:
                break

        # Calculate final metrics
        final_state = daily_states[-1]
        final_pnl = final_state.position_pnl
        final_pnl_pct = final_state.position_pnl_pct
        max_loss = min(state.position_pnl for state in daily_states)
        max_gain = max(state.position_pnl for state in daily_states)

        # Count rehedges
        num_rehedges = sum(
            1 for i in range(1, len(daily_states))
            if (daily_states[i].hedge_position and
                daily_states[i-1].hedge_position and
                daily_states[i].hedge_position.shares_to_hold !=
                daily_states[i-1].hedge_position.shares_to_hold)
        )

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
                scenario.get_iv_at_step(
                    path.realized_volatility, i, len(path.spot_prices)
                )
                for i in range(len(path.spot_prices))
            ]),
            num_rehedges=num_rehedges,
            exit_day=final_state.day if final_state.should_exit else None,
            exit_reason=final_state.exit_reason,
        )

    def _calculate_position_price(
        self, spot: float, time: float, time_remaining: float, iv: float
    ) -> float:
        """Calculate total position price across all legs."""
        total_price = 0.0

        for leg in self.config.legs:
            leg_time_remaining = leg.expiration - time
            leg_time_remaining = max(leg_time_remaining, 0)

            if leg_time_remaining <= 0:
                # Intrinsic value
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
        self, spot: float, time: float, time_remaining: float, iv: float
    ) -> GreeksSummary:
        """Calculate total position Greeks across all legs."""
        total_delta = 0.0
        total_gamma = 0.0
        total_vega = 0.0
        total_theta = 0.0
        total_rho = 0.0

        for leg in self.config.legs:
            leg_time_remaining = leg.expiration - time
            if leg_time_remaining <= 0:
                continue

            leg_greeks = calculate_greeks(
                S=spot,
                K=leg.strike,
                T=leg_time_remaining,
                r=self.config.risk_free_rate,
                sigma=iv,
                option_type=leg.option_type,
            )

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

    def _calculate_pnl_breakdown(
        self, day: int, daily_states: List[DailyState],
        position_greeks: GreeksSummary, spot_price: float, iv: float,
        path: SimulationPath
    ) -> PnLBreakdown:
        """Calculate P&L attribution for the day."""
        if day == 0:
            return PnLBreakdown(theta=0, gamma=0, vega=0, delta=0)

        prev_state = daily_states[-1]
        spot_move = spot_price - prev_state.spot_price
        iv_move = iv - prev_state.implied_volatility
        time_decay = path.times[day] - path.times[day - 1]

        pnl_theta = prev_state.theta * time_decay
        pnl_delta = prev_state.delta * spot_move
        pnl_gamma = 0.5 * prev_state.gamma * (spot_move ** 2)
        pnl_vega = prev_state.vega * iv_move

        return PnLBreakdown(
            theta=pnl_theta,
            gamma=pnl_gamma,
            vega=pnl_vega,
            delta=pnl_delta,
        )

    def _check_exit_conditions(
        self, day: int, unrealized_pnl: float, unrealized_pnl_pct: float,
        time_remaining: float, total_days: int
    ) -> tuple:
        """Check if position should exit."""
        if self.config.exit_condition == ExitCondition.HOLD_TO_EXPIRY:
            if time_remaining <= 0:
                return True, "Expiration"

        elif self.config.exit_condition == ExitCondition.PROFIT_TARGET:
            if unrealized_pnl >= self.config.exit_param:
                return True, f"Profit target ${self.config.exit_param:.2f}"

        elif self.config.exit_condition == ExitCondition.STOP_LOSS:
            if unrealized_pnl <= -self.config.exit_param:
                return True, f"Stop loss ${self.config.exit_param:.2f}"

        elif self.config.exit_condition == ExitCondition.TIME_TARGET:
            if day >= self.config.exit_param:
                return True, f"Time target {int(self.config.exit_param)} days"

        # Check absolute max loss
        if self.config.max_loss is not None:
            if unrealized_pnl <= -self.config.max_loss:
                return True, f"Max loss ${self.config.max_loss:.2f}"

        # Check expiry
        if time_remaining <= 0:
            return True, "Expiration"

        return False, None
