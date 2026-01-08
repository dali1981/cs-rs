#!/usr/bin/env python3
"""
Configuration module for the simulation system.

Provides immutable, serializable configuration objects that define
simulation parameters, strategy definitions, and scenario settings.

Example:
    config = SimulationConfig(
        market=MarketConfig(spot_price=100, risk_free_rate=0.05),
        strategy=StrategyPresets.long_call_atm(),
        scenarios=[ScenarioPresets.iv_equals_rv(), ScenarioPresets.iv_greater_rv()],
        simulation=SimulationParams(num_simulations=1000, num_days=30),
    )

    # Run with different hedging modes
    results = engine.run(config, hedging_modes=[HedgingMode.NONE, HedgingMode.DAILY])
"""

from dataclasses import dataclass, field
from typing import List, Optional, Dict, Any, Tuple
from enum import Enum, auto
import json
from pathlib import Path


# ============================================================================
# ENUMS
# ============================================================================

class OptionType(Enum):
    """Option type."""
    CALL = "call"
    PUT = "put"


class PositionDirection(Enum):
    """Position direction."""
    LONG = "long"
    SHORT = "short"


class HedgingMode(Enum):
    """Delta hedging configuration."""
    NONE = "none"               # No hedging
    DAILY = "daily"             # Rehedge daily
    THRESHOLD = "threshold"     # Rehedge when delta drifts
    WEEKLY = "weekly"           # Rehedge weekly


class ScenarioType(Enum):
    """Volatility scenario types."""
    IV_EQUALS_RV = "iv_equals_rv"       # IV = Realized Vol
    IV_GREATER_RV = "iv_greater_rv"     # IV > Realized Vol (long loses)
    IV_LESS_RV = "iv_less_rv"           # IV < Realized Vol (long wins)
    IV_INCREASES = "iv_increases"       # IV increases during trade
    IV_CRUSH = "iv_crush"               # IV decreases (crush)
    CUSTOM = "custom"                   # Custom IV function


class ExitCondition(Enum):
    """Trade exit conditions."""
    HOLD_TO_EXPIRY = "hold"
    PROFIT_TARGET = "profit"
    STOP_LOSS = "stop"
    TIME_TARGET = "time"


# ============================================================================
# CONSTANTS
# ============================================================================

class ContractConstants:
    """Standard options contract parameters."""
    SHARES_PER_CONTRACT = 100
    TRADING_DAYS_PER_YEAR = 252


class HedgingDefaults:
    """Default hedging parameters."""
    REHEDGE_FREQUENCY = 1  # days
    DELTA_THRESHOLD = 0.05  # 5% drift
    WEEKLY_FREQUENCY = 5  # days


# ============================================================================
# CONFIGURATION CLASSES
# ============================================================================

@dataclass(frozen=True)
class MarketConfig:
    """
    Market parameters for simulation.

    Attributes:
        spot_price: Initial stock price
        risk_free_rate: Annual risk-free rate (e.g., 0.05 for 5%)
        dividend_yield: Annual dividend yield (default 0)
        entry_iv: Entry implied volatility (e.g., 0.25 for 25%)
    """
    spot_price: float = 100.0
    risk_free_rate: float = 0.05
    dividend_yield: float = 0.0
    entry_iv: float = 0.25

    def __post_init__(self):
        if self.spot_price <= 0:
            raise ValueError("spot_price must be positive")
        if self.risk_free_rate < 0:
            raise ValueError("risk_free_rate must be non-negative")
        if self.entry_iv <= 0:
            raise ValueError("entry_iv must be positive")


@dataclass(frozen=True)
class OptionLegConfig:
    """
    Configuration for a single option leg.

    Attributes:
        option_type: CALL or PUT
        strike_offset: Strike relative to spot (0 = ATM, +10 = 10 OTM, -10 = 10 ITM)
        strike_pct: Strike as % of spot (1.0 = ATM, 1.1 = 10% OTM call)
        direction: LONG or SHORT
        quantity: Number of contracts
        expiration_days: Days to expiration

    Note: Use either strike_offset OR strike_pct, not both.
          If neither specified, defaults to ATM (strike_pct=1.0)
    """
    option_type: OptionType
    direction: PositionDirection
    quantity: int = 1
    expiration_days: int = 30
    strike_offset: Optional[float] = None  # Absolute offset from spot
    strike_pct: Optional[float] = None     # Strike as % of spot

    def __post_init__(self):
        if self.quantity <= 0:
            raise ValueError("quantity must be positive")
        if self.expiration_days <= 0:
            raise ValueError("expiration_days must be positive")
        if self.strike_offset is not None and self.strike_pct is not None:
            raise ValueError("Use either strike_offset OR strike_pct, not both")

    def get_strike(self, spot_price: float) -> float:
        """Calculate actual strike price given spot."""
        if self.strike_offset is not None:
            return spot_price + self.strike_offset
        elif self.strike_pct is not None:
            return spot_price * self.strike_pct
        else:
            return spot_price  # ATM

    @property
    def position_sign(self) -> float:
        """+1 for long, -1 for short."""
        return 1.0 if self.direction == PositionDirection.LONG else -1.0


@dataclass(frozen=True)
class StrategyConfig:
    """
    Complete strategy configuration.

    Attributes:
        name: Human-readable strategy name
        legs: List of option legs
        entry_price: Optional fixed entry price (if None, computed from BS)
    """
    name: str
    legs: Tuple[OptionLegConfig, ...]
    entry_price: Optional[float] = None

    def __post_init__(self):
        if not self.legs:
            raise ValueError("Strategy must have at least one leg")

    @property
    def num_legs(self) -> int:
        return len(self.legs)

    @property
    def is_spread(self) -> bool:
        return self.num_legs > 1


@dataclass(frozen=True)
class ScenarioConfig:
    """
    Volatility scenario configuration.

    Attributes:
        name: Scenario name for display
        scenario_type: Type of scenario
        realized_vol_multiplier: RV = entry_iv × multiplier (for simple scenarios)
        iv_path_multiplier: How IV evolves (for IV_INCREASES/IV_CRUSH)
    """
    name: str
    scenario_type: ScenarioType
    realized_vol_multiplier: float = 1.0  # RV = entry_iv × this
    iv_evolution_rate: float = 0.0  # For IV changes over time

    def get_realized_vol(self, entry_iv: float) -> float:
        """Calculate realized volatility given entry IV."""
        return entry_iv * self.realized_vol_multiplier

    def get_iv_at_time(self, entry_iv: float, time_progress: float) -> float:
        """
        Get IV at a given point in the trade.

        Args:
            entry_iv: IV at entry
            time_progress: 0.0 (start) to 1.0 (expiry)

        Returns:
            IV at this point in time
        """
        if self.scenario_type == ScenarioType.IV_EQUALS_RV:
            return self.get_realized_vol(entry_iv)
        elif self.scenario_type == ScenarioType.IV_GREATER_RV:
            return entry_iv  # IV stays high, RV is lower
        elif self.scenario_type == ScenarioType.IV_LESS_RV:
            return entry_iv  # IV stays low, RV is higher
        elif self.scenario_type == ScenarioType.IV_INCREASES:
            return entry_iv * (1.0 + self.iv_evolution_rate * time_progress)
        elif self.scenario_type == ScenarioType.IV_CRUSH:
            return entry_iv * max(0.5, 1.0 - self.iv_evolution_rate * time_progress)
        else:
            return entry_iv


@dataclass(frozen=True)
class HedgingConfig:
    """
    Delta hedging configuration.

    Attributes:
        mode: Hedging mode (NONE, DAILY, THRESHOLD, WEEKLY)
        threshold: Delta drift threshold for THRESHOLD mode
        frequency: Rehedge frequency in days
    """
    mode: HedgingMode = HedgingMode.NONE
    threshold: float = HedgingDefaults.DELTA_THRESHOLD
    frequency: int = HedgingDefaults.REHEDGE_FREQUENCY

    @property
    def is_enabled(self) -> bool:
        return self.mode != HedgingMode.NONE


@dataclass(frozen=True)
class ExitConfig:
    """
    Trade exit configuration.

    Attributes:
        condition: Exit condition type
        profit_target: Profit target in dollars (for PROFIT_TARGET)
        stop_loss: Maximum loss in dollars (for STOP_LOSS)
        time_days: Exit after N days (for TIME_TARGET)
    """
    condition: ExitCondition = ExitCondition.HOLD_TO_EXPIRY
    profit_target: Optional[float] = None
    stop_loss: Optional[float] = None
    time_days: Optional[int] = None


@dataclass(frozen=True)
class SimulationParams:
    """
    Simulation execution parameters.

    Attributes:
        num_simulations: Number of Monte Carlo paths
        num_days: Trading days to simulate
        random_seed: Optional seed for reproducibility
        parallel: Whether to run simulations in parallel
    """
    num_simulations: int = 1000
    num_days: int = 30
    random_seed: Optional[int] = None
    parallel: bool = True

    def __post_init__(self):
        if self.num_simulations <= 0:
            raise ValueError("num_simulations must be positive")
        if self.num_days <= 0:
            raise ValueError("num_days must be positive")


@dataclass(frozen=True)
class SimulationConfig:
    """
    Complete simulation configuration.

    This is the main configuration object that ties everything together.

    Attributes:
        market: Market parameters
        strategy: Strategy definition
        scenarios: List of scenarios to run
        hedging_modes: List of hedging configurations to test
        simulation: Simulation parameters
        exit: Exit conditions

    Example:
        config = SimulationConfig(
            market=MarketConfig(spot_price=100, entry_iv=0.25),
            strategy=StrategyPresets.long_call_atm(),
            scenarios=[ScenarioPresets.iv_equals_rv()],
            hedging_modes=[HedgingConfig(HedgingMode.NONE), HedgingConfig(HedgingMode.DAILY)],
            simulation=SimulationParams(num_simulations=1000),
        )
    """
    market: MarketConfig
    strategy: StrategyConfig
    scenarios: Tuple[ScenarioConfig, ...]
    hedging_modes: Tuple[HedgingConfig, ...] = (HedgingConfig(HedgingMode.NONE),)
    simulation: SimulationParams = field(default_factory=SimulationParams)
    exit: ExitConfig = field(default_factory=ExitConfig)

    def __post_init__(self):
        if not self.scenarios:
            raise ValueError("At least one scenario required")

    @property
    def total_runs(self) -> int:
        """Total number of simulation runs (scenarios × hedging modes × num_simulations)."""
        return len(self.scenarios) * len(self.hedging_modes) * self.simulation.num_simulations

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization."""
        return {
            "market": {
                "spot_price": self.market.spot_price,
                "risk_free_rate": self.market.risk_free_rate,
                "dividend_yield": self.market.dividend_yield,
                "entry_iv": self.market.entry_iv,
            },
            "strategy": {
                "name": self.strategy.name,
                "legs": [
                    {
                        "option_type": leg.option_type.value,
                        "direction": leg.direction.value,
                        "quantity": leg.quantity,
                        "expiration_days": leg.expiration_days,
                        "strike_offset": leg.strike_offset,
                        "strike_pct": leg.strike_pct,
                    }
                    for leg in self.strategy.legs
                ],
            },
            "scenarios": [
                {
                    "name": s.name,
                    "scenario_type": s.scenario_type.value,
                    "realized_vol_multiplier": s.realized_vol_multiplier,
                }
                for s in self.scenarios
            ],
            "hedging_modes": [
                {
                    "mode": h.mode.value,
                    "threshold": h.threshold,
                    "frequency": h.frequency,
                }
                for h in self.hedging_modes
            ],
            "simulation": {
                "num_simulations": self.simulation.num_simulations,
                "num_days": self.simulation.num_days,
                "random_seed": self.simulation.random_seed,
            },
        }

    def save(self, path: Path) -> None:
        """Save configuration to JSON file."""
        with open(path, "w") as f:
            json.dump(self.to_dict(), f, indent=2)

    @classmethod
    def load(cls, path: Path) -> "SimulationConfig":
        """Load configuration from JSON file."""
        with open(path, "r") as f:
            data = json.load(f)

        return cls(
            market=MarketConfig(**data["market"]),
            strategy=StrategyConfig(
                name=data["strategy"]["name"],
                legs=tuple(
                    OptionLegConfig(
                        option_type=OptionType(leg["option_type"]),
                        direction=PositionDirection(leg["direction"]),
                        quantity=leg["quantity"],
                        expiration_days=leg["expiration_days"],
                        strike_offset=leg.get("strike_offset"),
                        strike_pct=leg.get("strike_pct"),
                    )
                    for leg in data["strategy"]["legs"]
                ),
            ),
            scenarios=tuple(
                ScenarioConfig(
                    name=s["name"],
                    scenario_type=ScenarioType(s["scenario_type"]),
                    realized_vol_multiplier=s.get("realized_vol_multiplier", 1.0),
                )
                for s in data["scenarios"]
            ),
            hedging_modes=tuple(
                HedgingConfig(
                    mode=HedgingMode(h["mode"]),
                    threshold=h.get("threshold", 0.05),
                    frequency=h.get("frequency", 1),
                )
                for h in data.get("hedging_modes", [{"mode": "none"}])
            ),
            simulation=SimulationParams(
                num_simulations=data["simulation"]["num_simulations"],
                num_days=data["simulation"]["num_days"],
                random_seed=data["simulation"].get("random_seed"),
            ),
        )


# ============================================================================
# PRESETS - Common configurations
# ============================================================================

class StrategyPresets:
    """Predefined strategy configurations."""

    @staticmethod
    def long_call_atm() -> StrategyConfig:
        """Long ATM call."""
        return StrategyConfig(
            name="Long Call (ATM)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                ),
            ),
        )

    @staticmethod
    def short_call_atm() -> StrategyConfig:
        """Short ATM call."""
        return StrategyConfig(
            name="Short Call (ATM)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                ),
            ),
        )

    @staticmethod
    def long_put_atm() -> StrategyConfig:
        """Long ATM put."""
        return StrategyConfig(
            name="Long Put (ATM)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.PUT,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                ),
            ),
        )

    @staticmethod
    def short_put_atm() -> StrategyConfig:
        """Short ATM put."""
        return StrategyConfig(
            name="Short Put (ATM)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.PUT,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                ),
            ),
        )

    @staticmethod
    def long_straddle_atm() -> StrategyConfig:
        """Long ATM straddle (long call + long put)."""
        return StrategyConfig(
            name="Long Straddle (ATM)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                ),
                OptionLegConfig(
                    option_type=OptionType.PUT,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                ),
            ),
        )

    @staticmethod
    def short_straddle_atm() -> StrategyConfig:
        """Short ATM straddle (short call + short put)."""
        return StrategyConfig(
            name="Short Straddle (ATM)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                ),
                OptionLegConfig(
                    option_type=OptionType.PUT,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                ),
            ),
        )

    @staticmethod
    def bull_call_spread(width_pct: float = 0.05) -> StrategyConfig:
        """
        Bull call spread.

        Args:
            width_pct: Width of spread as % of spot (default 5%)
        """
        return StrategyConfig(
            name=f"Bull Call Spread ({width_pct*100:.0f}%)",
            legs=(
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                    strike_pct=1.0,  # ATM
                ),
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                    strike_pct=1.0 + width_pct,  # OTM
                ),
            ),
        )

    @staticmethod
    def iron_condor(wing_width_pct: float = 0.10) -> StrategyConfig:
        """
        Iron condor (short straddle + long wings).

        Args:
            wing_width_pct: Wing distance as % of spot (default 10%)
        """
        return StrategyConfig(
            name=f"Iron Condor ({wing_width_pct*100:.0f}% wings)",
            legs=(
                # Short call (ATM)
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                    strike_pct=1.0,
                ),
                # Short put (ATM)
                OptionLegConfig(
                    option_type=OptionType.PUT,
                    direction=PositionDirection.SHORT,
                    quantity=1,
                    expiration_days=30,
                    strike_pct=1.0,
                ),
                # Long call (OTM wing)
                OptionLegConfig(
                    option_type=OptionType.CALL,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                    strike_pct=1.0 + wing_width_pct,
                ),
                # Long put (OTM wing)
                OptionLegConfig(
                    option_type=OptionType.PUT,
                    direction=PositionDirection.LONG,
                    quantity=1,
                    expiration_days=30,
                    strike_pct=1.0 - wing_width_pct,
                ),
            ),
        )


class ScenarioPresets:
    """Predefined scenario configurations."""

    @staticmethod
    def iv_equals_rv() -> ScenarioConfig:
        """IV = Realized Vol (fair pricing)."""
        return ScenarioConfig(
            name="IV = RV",
            scenario_type=ScenarioType.IV_EQUALS_RV,
            realized_vol_multiplier=1.0,
        )

    @staticmethod
    def iv_greater_rv(multiplier: float = 0.75) -> ScenarioConfig:
        """
        IV > RV (options overpriced, short wins).

        Args:
            multiplier: RV = IV × multiplier (default 0.75 = 25% less)
        """
        return ScenarioConfig(
            name=f"IV > RV ({(1-multiplier)*100:.0f}% less RV)",
            scenario_type=ScenarioType.IV_GREATER_RV,
            realized_vol_multiplier=multiplier,
        )

    @staticmethod
    def iv_less_rv(multiplier: float = 1.25) -> ScenarioConfig:
        """
        IV < RV (options underpriced, long wins).

        Args:
            multiplier: RV = IV × multiplier (default 1.25 = 25% more)
        """
        return ScenarioConfig(
            name=f"IV < RV ({(multiplier-1)*100:.0f}% more RV)",
            scenario_type=ScenarioType.IV_LESS_RV,
            realized_vol_multiplier=multiplier,
        )

    @staticmethod
    def iv_crush(crush_rate: float = 0.5) -> ScenarioConfig:
        """
        IV crush scenario (IV decreases during trade).

        Args:
            crush_rate: How much IV drops (0.5 = drops to 50%)
        """
        return ScenarioConfig(
            name=f"IV Crush ({crush_rate*100:.0f}%)",
            scenario_type=ScenarioType.IV_CRUSH,
            realized_vol_multiplier=1.0,
            iv_evolution_rate=crush_rate,
        )

    @staticmethod
    def iv_spike(spike_rate: float = 0.5) -> ScenarioConfig:
        """
        IV spike scenario (IV increases during trade).

        Args:
            spike_rate: How much IV rises (0.5 = rises 50%)
        """
        return ScenarioConfig(
            name=f"IV Spike ({spike_rate*100:.0f}%)",
            scenario_type=ScenarioType.IV_INCREASES,
            realized_vol_multiplier=1.0,
            iv_evolution_rate=spike_rate,
        )

    @staticmethod
    def all_standard() -> Tuple[ScenarioConfig, ...]:
        """All standard scenarios for comprehensive testing."""
        return (
            ScenarioPresets.iv_equals_rv(),
            ScenarioPresets.iv_greater_rv(0.75),
            ScenarioPresets.iv_less_rv(1.25),
            ScenarioPresets.iv_crush(0.5),
            ScenarioPresets.iv_spike(0.5),
        )


class HedgingPresets:
    """Predefined hedging configurations."""

    @staticmethod
    def no_hedge() -> HedgingConfig:
        """No delta hedging."""
        return HedgingConfig(mode=HedgingMode.NONE)

    @staticmethod
    def daily_hedge() -> HedgingConfig:
        """Daily delta hedging."""
        return HedgingConfig(mode=HedgingMode.DAILY, frequency=1)

    @staticmethod
    def weekly_hedge() -> HedgingConfig:
        """Weekly delta hedging."""
        return HedgingConfig(mode=HedgingMode.WEEKLY, frequency=5)

    @staticmethod
    def threshold_hedge(threshold: float = 0.05) -> HedgingConfig:
        """Threshold-based hedging (rehedge when delta drifts)."""
        return HedgingConfig(mode=HedgingMode.THRESHOLD, threshold=threshold)

    @staticmethod
    def all_modes() -> Tuple[HedgingConfig, ...]:
        """All hedging modes for comparison."""
        return (
            HedgingPresets.no_hedge(),
            HedgingPresets.daily_hedge(),
            HedgingPresets.weekly_hedge(),
        )


# ============================================================================
# QUICK CONFIG BUILDERS
# ============================================================================

def quick_config(
    strategy: str = "long_call",
    spot: float = 100.0,
    iv: float = 0.25,
    days: int = 30,
    num_sims: int = 1000,
    scenarios: str = "all",
    hedging: str = "both",
    seed: Optional[int] = None,
) -> SimulationConfig:
    """
    Quick configuration builder for common setups.

    Args:
        strategy: Strategy name ("long_call", "short_call", "straddle", etc.)
        spot: Initial spot price
        iv: Entry implied volatility
        days: Days to expiration
        num_sims: Number of simulations
        scenarios: "all", "iv_equals_rv", "iv_greater_rv", "iv_less_rv"
        hedging: "none", "daily", "both", "all"
        seed: Random seed for reproducibility

    Returns:
        Complete SimulationConfig

    Example:
        config = quick_config(
            strategy="long_call",
            spot=100,
            iv=0.25,
            num_sims=1000,
            scenarios="all",
            hedging="both",
        )
    """
    # Strategy
    strategy_map = {
        "long_call": StrategyPresets.long_call_atm,
        "short_call": StrategyPresets.short_call_atm,
        "long_put": StrategyPresets.long_put_atm,
        "short_put": StrategyPresets.short_put_atm,
        "long_straddle": StrategyPresets.long_straddle_atm,
        "short_straddle": StrategyPresets.short_straddle_atm,
        "bull_call_spread": StrategyPresets.bull_call_spread,
        "iron_condor": StrategyPresets.iron_condor,
    }
    strategy_config = strategy_map.get(strategy, StrategyPresets.long_call_atm)()

    # Update expiration days in strategy
    updated_legs = tuple(
        OptionLegConfig(
            option_type=leg.option_type,
            direction=leg.direction,
            quantity=leg.quantity,
            expiration_days=days,
            strike_offset=leg.strike_offset,
            strike_pct=leg.strike_pct,
        )
        for leg in strategy_config.legs
    )
    strategy_config = StrategyConfig(name=strategy_config.name, legs=updated_legs)

    # Scenarios
    scenario_map = {
        "all": ScenarioPresets.all_standard,
        "standard": lambda: (ScenarioPresets.iv_equals_rv(), ScenarioPresets.iv_greater_rv(), ScenarioPresets.iv_less_rv()),
        "iv_equals_rv": lambda: (ScenarioPresets.iv_equals_rv(),),
        "iv_greater_rv": lambda: (ScenarioPresets.iv_greater_rv(),),
        "iv_less_rv": lambda: (ScenarioPresets.iv_less_rv(),),
    }
    scenario_configs = scenario_map.get(scenarios, lambda: (ScenarioPresets.iv_equals_rv(),))()

    # Hedging
    hedging_map = {
        "none": lambda: (HedgingPresets.no_hedge(),),
        "daily": lambda: (HedgingPresets.daily_hedge(),),
        "weekly": lambda: (HedgingPresets.weekly_hedge(),),
        "both": lambda: (HedgingPresets.no_hedge(), HedgingPresets.daily_hedge()),
        "all": HedgingPresets.all_modes,
    }
    hedging_configs = hedging_map.get(hedging, lambda: (HedgingPresets.no_hedge(),))()

    return SimulationConfig(
        market=MarketConfig(spot_price=spot, entry_iv=iv),
        strategy=strategy_config,
        scenarios=scenario_configs,
        hedging_modes=hedging_configs,
        simulation=SimulationParams(num_simulations=num_sims, num_days=days, random_seed=seed),
    )
