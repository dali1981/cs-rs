#!/usr/bin/env python3
"""
Core stock price simulation using GBM and Heston models.

Supports:
- Geometric Brownian Motion (Black-Scholes model)
- Heston Stochastic Volatility model
"""

import numpy as np
from dataclasses import dataclass
from typing import Tuple
from enum import Enum


class SimulationModel(Enum):
    """Available simulation models."""
    GBM = "gbm"  # Geometric Brownian Motion (BSM)
    HESTON = "heston"  # Stochastic Volatility


@dataclass
class GBMConfig:
    """Configuration for GBM simulation."""
    spot_price: float  # S0
    drift_rate: float  # mu (annualized)
    volatility: float  # sigma (annualized)

    def __post_init__(self):
        if self.spot_price <= 0:
            raise ValueError("spot_price must be positive")
        if self.volatility <= 0:
            raise ValueError("volatility must be positive")


@dataclass
class HestonConfig:
    """Configuration for Heston simulation."""
    spot_price: float  # S0
    drift_rate: float  # mu (annualized)
    initial_variance: float  # v0 (volatility^2)
    mean_variance: float  # theta (long-term variance)
    variance_of_variance: float  # xi (vol of vol)
    mean_reversion: float  # kappa (mean reversion speed)
    rho: float  # correlation between stock and vol processes (-1 to 1)

    def __post_init__(self):
        if self.spot_price <= 0:
            raise ValueError("spot_price must be positive")
        if self.initial_variance <= 0:
            raise ValueError("initial_variance must be positive")
        if self.variance_of_variance <= 0:
            raise ValueError("variance_of_variance must be positive")
        if not -1 <= self.rho <= 1:
            raise ValueError("rho must be between -1 and 1")


@dataclass
class SimulationPath:
    """Result of stock simulation."""
    times: np.ndarray  # Trading days (0 to T)
    spot_prices: np.ndarray  # Stock prices over time
    log_returns: np.ndarray  # Log returns at each step
    realized_volatility: float  # Annualized realized volatility
    realized_variance: float  # Annualized realized variance

    @property
    def length(self) -> int:
        return len(self.spot_prices)

    @property
    def initial_price(self) -> float:
        return self.spot_prices[0]

    @property
    def final_price(self) -> float:
        return self.spot_prices[-1]

    @property
    def final_return(self) -> float:
        return (self.final_price - self.initial_price) / self.initial_price


class StockSimulator:
    """Simulates stock price paths using various models."""

    @staticmethod
    def simulate_gbm(
        config: GBMConfig,
        time_to_expiry: float,
        num_steps: int,
        num_paths: int = 1,
        random_seed: int = None,
    ) -> Tuple[SimulationPath, np.ndarray]:
        """
        Simulate stock prices using Geometric Brownian Motion.

        Parameters:
        -----------
        config : GBMConfig
            GBM configuration
        time_to_expiry : float
            Time to expiration in years (e.g., 30 days = 30/365)
        num_steps : int
            Number of time steps in simulation
        num_paths : int
            Number of independent paths to simulate
        random_seed : int
            Random seed for reproducibility

        Returns:
        --------
        (SimulationPath, np.ndarray)
            - Single representative path (median)
            - All simulated paths (num_paths x num_steps)
        """
        if random_seed is not None:
            np.random.seed(random_seed)

        dt = time_to_expiry / num_steps
        times = np.linspace(0, time_to_expiry, num_steps)

        # Initialize price matrix
        paths = np.zeros((num_paths, num_steps))
        paths[:, 0] = config.spot_price

        # Generate random increments: dW ~ N(0, dt)
        dW = np.random.normal(0, np.sqrt(dt), (num_paths, num_steps - 1))

        # Simulate paths
        for i in range(num_steps - 1):
            # dS/S = mu*dt + sigma*dW
            # S(t+dt) = S(t) * exp((mu - sigma^2/2)*dt + sigma*dW)
            paths[:, i + 1] = paths[:, i] * np.exp(
                (config.drift_rate - 0.5 * config.volatility ** 2) * dt
                + config.volatility * dW[:, i]
            )

        # Compute median path as representative
        median_path = np.median(paths, axis=0)

        # Compute log returns for realized vol
        log_returns = np.log(median_path[1:] / median_path[:-1])

        # Annualized realized volatility
        realized_vol = np.std(log_returns) * np.sqrt(252)  # 252 trading days
        realized_var = realized_vol ** 2

        result = SimulationPath(
            times=times,
            spot_prices=median_path,
            log_returns=log_returns,
            realized_volatility=realized_vol,
            realized_variance=realized_var,
        )

        return result, paths

    @staticmethod
    def simulate_heston(
        config: HestonConfig,
        time_to_expiry: float,
        num_steps: int,
        num_paths: int = 1,
        random_seed: int = None,
    ) -> Tuple[SimulationPath, np.ndarray]:
        """
        Simulate stock prices using Heston Stochastic Volatility model.

        dS/S = mu*dt + sqrt(v)*dW_S
        dv = kappa*(theta - v)*dt + xi*sqrt(v)*dW_v

        where dW_S and dW_v are correlated Brownian motions with correlation rho.

        Parameters:
        -----------
        config : HestonConfig
            Heston model configuration
        time_to_expiry : float
            Time to expiration in years
        num_steps : int
            Number of time steps
        num_paths : int
            Number of independent paths
        random_seed : int
            Random seed for reproducibility

        Returns:
        --------
        (SimulationPath, np.ndarray)
            Simulated paths with stochastic volatility
        """
        if random_seed is not None:
            np.random.seed(random_seed)

        dt = time_to_expiry / num_steps
        times = np.linspace(0, time_to_expiry, num_steps)

        # Initialize arrays
        prices = np.zeros((num_paths, num_steps))
        variances = np.zeros((num_paths, num_steps))

        prices[:, 0] = config.spot_price
        variances[:, 0] = config.initial_variance

        # Generate correlated Brownian motions
        dW_S_raw = np.random.normal(0, 1, (num_paths, num_steps - 1))
        dW_v_raw = np.random.normal(0, 1, (num_paths, num_steps - 1))

        # Correlate: dW_v = rho*dW_S + sqrt(1-rho^2)*dW_v_raw
        dW_v = (
            config.rho * dW_S_raw +
            np.sqrt(max(0, 1 - config.rho ** 2)) * dW_v_raw
        )

        sqrt_dt = np.sqrt(dt)

        # Simulate paths using Euler discretization
        for i in range(num_steps - 1):
            sqrt_v = np.sqrt(np.maximum(variances[:, i], 0.0001))  # Avoid negative variance

            # dv = kappa*(theta - v)*dt + xi*sqrt(v)*dW_v
            variances[:, i + 1] = np.maximum(
                variances[:, i] + config.mean_reversion * (config.mean_variance - variances[:, i]) * dt
                + config.variance_of_variance * sqrt_v * dW_v[:, i] * sqrt_dt,
                0.0001  # Floor to avoid negative variance
            )

            # dS/S = mu*dt + sqrt(v)*dW_S
            prices[:, i + 1] = prices[:, i] * np.exp(
                (config.drift_rate - 0.5 * variances[:, i]) * dt
                + sqrt_v * dW_S_raw[:, i] * sqrt_dt
            )

        # Median path
        median_path = np.median(prices, axis=0)

        # Compute realized vol
        log_returns = np.log(median_path[1:] / median_path[:-1])
        realized_vol = np.std(log_returns) * np.sqrt(252)
        realized_var = realized_vol ** 2

        result = SimulationPath(
            times=times,
            spot_prices=median_path,
            log_returns=log_returns,
            realized_volatility=realized_vol,
            realized_variance=realized_var,
        )

        return result, prices


class VolatilityScenario:
    """Defines how implied volatility evolves during the simulation."""

    def __init__(self, name: str, iv_func):
        """
        Parameters:
        -----------
        name : str
            Scenario name (e.g., "IV equals RV")
        iv_func : callable
            Function(realized_vol, time_progress) -> implied_vol at each step
        """
        self.name = name
        self.iv_func = iv_func

    def get_iv_at_step(self, realized_vol: float, step: int, total_steps: int) -> float:
        """Get implied volatility at a specific step."""
        time_progress = step / total_steps  # 0 to 1
        return self.iv_func(realized_vol, time_progress)


# Predefined scenarios
SCENARIO_IV_EQUALS_RV = VolatilityScenario(
    "IV equals RV",
    lambda rv, t: rv
)

SCENARIO_IV_GREATER_RV = VolatilityScenario(
    "IV > RV (Vega Win)",
    lambda rv, t: rv * 1.2  # IV 20% higher than realized
)

SCENARIO_IV_LESS_RV = VolatilityScenario(
    "IV < RV (Vega Loss)",
    lambda rv, t: rv * 0.8  # IV 20% lower than realized
)

SCENARIO_IV_INCREASES = VolatilityScenario(
    "IV Increases",
    lambda rv, t: rv * (1.0 + 0.5 * t)  # IV increases linearly over time
)

SCENARIO_IV_CRUSH = VolatilityScenario(
    "IV Crush",
    lambda rv, t: rv * max(0.5, 1.0 - 0.5 * t)  # IV decreases linearly
)
