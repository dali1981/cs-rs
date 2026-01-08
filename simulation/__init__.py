"""Trade simulation package with scenario analysis and delta hedging."""

from .core_simulator import (
    StockSimulator,
    GBMConfig,
    HestonConfig,
    SimulationPath,
    VolatilityScenario,
    SimulationModel,
    SCENARIO_IV_EQUALS_RV,
    SCENARIO_IV_GREATER_RV,
    SCENARIO_IV_LESS_RV,
    SCENARIO_IV_INCREASES,
    SCENARIO_IV_CRUSH,
)

from .black_scholes import (
    BlackScholes,
    OptionType,
    calculate_greeks,
    GreeksSummary,
)

from .strategy_simulator import (
    StrategySimulator,
    StrategyConfig,
    OptionLeg,
    ExitCondition,
    SimulationResult,
    DailyState,
)

__all__ = [
    # Core simulator
    "StockSimulator",
    "GBMConfig",
    "HestonConfig",
    "SimulationPath",
    "VolatilityScenario",
    "SimulationModel",
    # Volatility scenarios
    "SCENARIO_IV_EQUALS_RV",
    "SCENARIO_IV_GREATER_RV",
    "SCENARIO_IV_LESS_RV",
    "SCENARIO_IV_INCREASES",
    "SCENARIO_IV_CRUSH",
    # Black-Scholes
    "BlackScholes",
    "OptionType",
    "calculate_greeks",
    "GreeksSummary",
    # Strategy simulation
    "StrategySimulator",
    "StrategyConfig",
    "OptionLeg",
    "ExitCondition",
    "SimulationResult",
    "DailyState",
]
