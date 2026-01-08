#!/usr/bin/env python3
"""
Simulation Engine - Runs multiple option strategy simulations.

This module provides the core simulation engine that:
1. Takes a SimulationConfig
2. Generates multiple stock price paths
3. Simulates option strategies with various hedging modes
4. Returns aggregated results

Example:
    from config import quick_config
    from engine import SimulationEngine

    config = quick_config(strategy="long_call", num_sims=1000)
    engine = SimulationEngine()
    results = engine.run(config)

    print(f"Mean P&L: ${results.mean_pnl:.2f}")
    print(f"Win Rate: {results.win_rate:.1%}")
"""

import numpy as np
from dataclasses import dataclass, field
from typing import List, Dict, Optional, Tuple, Callable
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor
import multiprocessing
from tqdm import tqdm

from config import (
    SimulationConfig, MarketConfig, StrategyConfig, ScenarioConfig,
    HedgingConfig, HedgingMode, OptionLegConfig, OptionType, PositionDirection,
    ContractConstants,
)


# ============================================================================
# BLACK-SCHOLES PRICING (Inline for engine independence)
# ============================================================================

def _norm_cdf(x: float) -> float:
    """Standard normal CDF using error function."""
    from math import erf, sqrt
    return 0.5 * (1 + erf(x / sqrt(2)))


def _norm_pdf(x: float) -> float:
    """Standard normal PDF."""
    from math import exp, sqrt, pi
    return exp(-0.5 * x * x) / sqrt(2 * pi)


def bs_price(S: float, K: float, T: float, r: float, sigma: float, is_call: bool) -> float:
    """Black-Scholes option price."""
    if T <= 0:
        # Intrinsic value
        if is_call:
            return max(S - K, 0)
        return max(K - S, 0)

    from math import log, sqrt, exp

    d1 = (log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * sqrt(T))
    d2 = d1 - sigma * sqrt(T)

    if is_call:
        return S * _norm_cdf(d1) - K * exp(-r * T) * _norm_cdf(d2)
    return K * exp(-r * T) * _norm_cdf(-d2) - S * _norm_cdf(-d1)


def bs_delta(S: float, K: float, T: float, r: float, sigma: float, is_call: bool) -> float:
    """Black-Scholes delta."""
    if T <= 0:
        if is_call:
            return 1.0 if S > K else 0.0
        return -1.0 if S < K else 0.0

    from math import log, sqrt

    d1 = (log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * sqrt(T))

    if is_call:
        return _norm_cdf(d1)
    return _norm_cdf(d1) - 1


def bs_gamma(S: float, K: float, T: float, r: float, sigma: float) -> float:
    """Black-Scholes gamma."""
    if T <= 0:
        return 0.0

    from math import log, sqrt

    d1 = (log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * sqrt(T))
    return _norm_pdf(d1) / (S * sigma * sqrt(T))


def bs_theta(S: float, K: float, T: float, r: float, sigma: float, is_call: bool) -> float:
    """Black-Scholes theta (per day)."""
    if T <= 0:
        return 0.0

    from math import log, sqrt, exp

    d1 = (log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * sqrt(T))
    d2 = d1 - sigma * sqrt(T)

    theta_common = -S * _norm_pdf(d1) * sigma / (2 * sqrt(T))

    if is_call:
        theta = theta_common - r * K * exp(-r * T) * _norm_cdf(d2)
    else:
        theta = theta_common + r * K * exp(-r * T) * _norm_cdf(-d2)

    return theta / ContractConstants.TRADING_DAYS_PER_YEAR


def bs_vega(S: float, K: float, T: float, r: float, sigma: float) -> float:
    """Black-Scholes vega."""
    if T <= 0:
        return 0.0

    from math import log, sqrt

    d1 = (log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * sqrt(T))
    return S * _norm_pdf(d1) * sqrt(T) / 100  # Per 1% IV change


# ============================================================================
# STOCK PATH GENERATION
# ============================================================================

def generate_gbm_paths(
    spot: float,
    drift: float,
    volatility: float,
    days: int,
    num_paths: int,
    seed: Optional[int] = None,
) -> np.ndarray:
    """
    Generate GBM stock price paths.

    Args:
        spot: Initial spot price
        drift: Annual drift rate
        volatility: Annual volatility
        days: Number of trading days
        num_paths: Number of paths to generate
        seed: Random seed

    Returns:
        Array of shape (num_paths, days+1) with spot prices
    """
    if seed is not None:
        np.random.seed(seed)

    dt = 1 / ContractConstants.TRADING_DAYS_PER_YEAR
    paths = np.zeros((num_paths, days + 1))
    paths[:, 0] = spot

    # Generate all random draws at once
    z = np.random.standard_normal((num_paths, days))

    for t in range(days):
        paths[:, t + 1] = paths[:, t] * np.exp(
            (drift - 0.5 * volatility**2) * dt + volatility * np.sqrt(dt) * z[:, t]
        )

    return paths


# ============================================================================
# SINGLE SIMULATION RESULT
# ============================================================================

@dataclass
class SingleSimResult:
    """Result of a single simulation run."""
    path_id: int
    scenario_name: str
    hedging_mode: str
    strategy_name: str

    # P&L metrics
    final_pnl: float
    final_pnl_pct: float
    max_gain: float
    max_loss: float

    # Spot path metrics
    final_spot: float
    spot_return: float
    realized_vol: float

    # Hedging metrics
    num_rehedges: int = 0
    total_hedge_cost: float = 0.0

    # Greeks at entry
    entry_delta: float = 0.0
    entry_gamma: float = 0.0
    entry_vega: float = 0.0
    entry_theta: float = 0.0

    @property
    def is_winner(self) -> bool:
        return self.final_pnl > 0

    @property
    def is_loser(self) -> bool:
        return self.final_pnl < 0


# ============================================================================
# AGGREGATED RESULTS
# ============================================================================

@dataclass
class AggregatedResults:
    """Aggregated results from multiple simulation runs."""
    config_name: str
    scenario_name: str
    hedging_mode: str
    strategy_name: str
    num_simulations: int

    # P&L distribution
    pnls: np.ndarray
    pnl_pcts: np.ndarray

    # Summary stats
    mean_pnl: float = 0.0
    std_pnl: float = 0.0
    median_pnl: float = 0.0
    min_pnl: float = 0.0
    max_pnl: float = 0.0

    # Percentiles
    pnl_5th: float = 0.0
    pnl_25th: float = 0.0
    pnl_75th: float = 0.0
    pnl_95th: float = 0.0

    # Win/loss metrics
    win_rate: float = 0.0
    avg_win: float = 0.0
    avg_loss: float = 0.0
    profit_factor: float = 0.0

    # Sharpe-like metrics
    sharpe_ratio: float = 0.0
    sortino_ratio: float = 0.0

    # Greeks averages
    avg_entry_delta: float = 0.0
    avg_entry_gamma: float = 0.0

    # Hedging metrics
    avg_rehedges: float = 0.0

    def __post_init__(self):
        """Compute summary statistics."""
        if len(self.pnls) == 0:
            return

        # Basic stats
        self.mean_pnl = float(np.mean(self.pnls))
        self.std_pnl = float(np.std(self.pnls))
        self.median_pnl = float(np.median(self.pnls))
        self.min_pnl = float(np.min(self.pnls))
        self.max_pnl = float(np.max(self.pnls))

        # Percentiles
        self.pnl_5th = float(np.percentile(self.pnls, 5))
        self.pnl_25th = float(np.percentile(self.pnls, 25))
        self.pnl_75th = float(np.percentile(self.pnls, 75))
        self.pnl_95th = float(np.percentile(self.pnls, 95))

        # Win/loss
        winners = self.pnls[self.pnls > 0]
        losers = self.pnls[self.pnls < 0]

        self.win_rate = len(winners) / len(self.pnls) if len(self.pnls) > 0 else 0
        self.avg_win = float(np.mean(winners)) if len(winners) > 0 else 0
        self.avg_loss = float(np.mean(losers)) if len(losers) > 0 else 0

        total_wins = float(np.sum(winners)) if len(winners) > 0 else 0
        total_losses = abs(float(np.sum(losers))) if len(losers) > 0 else 0
        self.profit_factor = total_wins / total_losses if total_losses > 0 else float('inf')

        # Risk-adjusted returns
        self.sharpe_ratio = self.mean_pnl / self.std_pnl if self.std_pnl > 0 else 0

        downside = self.pnls[self.pnls < 0]
        downside_std = float(np.std(downside)) if len(downside) > 0 else 0
        self.sortino_ratio = self.mean_pnl / downside_std if downside_std > 0 else 0

    def summary(self) -> str:
        """Return formatted summary string."""
        return f"""
{self.strategy_name} | {self.scenario_name} | Hedging: {self.hedging_mode}
{'─' * 60}
Simulations: {self.num_simulations:,}

P&L Distribution:
  Mean:     ${self.mean_pnl:>8.2f}  │  Std:    ${self.std_pnl:>8.2f}
  Median:   ${self.median_pnl:>8.2f}  │  Min:    ${self.min_pnl:>8.2f}
  Max:      ${self.max_pnl:>8.2f}

Percentiles:
  5th:      ${self.pnl_5th:>8.2f}  │  95th:   ${self.pnl_95th:>8.2f}
  25th:     ${self.pnl_25th:>8.2f}  │  75th:   ${self.pnl_75th:>8.2f}

Performance:
  Win Rate: {self.win_rate:>7.1%}   │  Profit Factor: {self.profit_factor:>6.2f}
  Avg Win:  ${self.avg_win:>8.2f}  │  Avg Loss: ${self.avg_loss:>8.2f}

Risk Metrics:
  Sharpe:   {self.sharpe_ratio:>7.2f}   │  Sortino: {self.sortino_ratio:>7.2f}
"""


# ============================================================================
# SIMULATION ENGINE
# ============================================================================

class SimulationEngine:
    """
    Core simulation engine for running Monte Carlo option simulations.

    Features:
    - Runs multiple paths with different scenarios and hedging modes
    - Supports parallel execution
    - Provides detailed and aggregated results

    Example:
        engine = SimulationEngine()
        config = quick_config(strategy="long_call", num_sims=1000)
        results = engine.run(config)

        for result in results:
            print(result.summary())
    """

    def __init__(self, progress_bar: bool = True, max_workers: Optional[int] = None):
        """
        Initialize simulation engine.

        Args:
            progress_bar: Show progress bar during simulation
            max_workers: Max parallel workers (None = CPU count)
        """
        self.progress_bar = progress_bar
        self.max_workers = max_workers or max(1, multiprocessing.cpu_count() - 1)

    def run(self, config: SimulationConfig) -> List[AggregatedResults]:
        """
        Run complete simulation suite.

        Args:
            config: Complete simulation configuration

        Returns:
            List of AggregatedResults, one per (scenario, hedging_mode) combination
        """
        results = []

        # Run each scenario × hedging mode combination
        total_combos = len(config.scenarios) * len(config.hedging_modes)

        if self.progress_bar:
            pbar = tqdm(total=total_combos, desc="Running simulations")

        for scenario in config.scenarios:
            # Generate paths with scenario-specific realized volatility
            # RV = entry_iv × realized_vol_multiplier
            scenario_rv = scenario.get_realized_vol(config.market.entry_iv)

            paths = generate_gbm_paths(
                spot=config.market.spot_price,
                drift=config.market.risk_free_rate,
                volatility=scenario_rv,  # Use scenario's realized vol for path generation
                days=config.simulation.num_days,
                num_paths=config.simulation.num_simulations,
                seed=config.simulation.random_seed,
            )

            for hedging in config.hedging_modes:
                # Run all paths for this combination
                single_results = self._run_paths(
                    paths=paths,
                    config=config,
                    scenario=scenario,
                    hedging=hedging,
                )

                # Aggregate results
                agg = self._aggregate_results(
                    single_results=single_results,
                    config=config,
                    scenario=scenario,
                    hedging=hedging,
                )
                results.append(agg)

                if self.progress_bar:
                    pbar.update(1)

        if self.progress_bar:
            pbar.close()

        return results

    def _run_paths(
        self,
        paths: np.ndarray,
        config: SimulationConfig,
        scenario: ScenarioConfig,
        hedging: HedgingConfig,
    ) -> List[SingleSimResult]:
        """Run simulation for all paths with given scenario and hedging."""
        results = []

        for path_id in range(len(paths)):
            result = self._simulate_single_path(
                path=paths[path_id],
                path_id=path_id,
                config=config,
                scenario=scenario,
                hedging=hedging,
            )
            results.append(result)

        return results

    def _simulate_single_path(
        self,
        path: np.ndarray,
        path_id: int,
        config: SimulationConfig,
        scenario: ScenarioConfig,
        hedging: HedgingConfig,
    ) -> SingleSimResult:
        """Simulate a single path."""
        market = config.market
        strategy = config.strategy

        # Calculate entry price if not specified
        entry_price = strategy.entry_price
        if entry_price is None:
            entry_price = self._calculate_strategy_price(
                spot=market.spot_price,
                strategy=strategy,
                market=market,
                time_years=config.simulation.num_days / ContractConstants.TRADING_DAYS_PER_YEAR,
            )

        # Calculate entry Greeks
        entry_delta, entry_gamma, entry_vega, entry_theta = self._calculate_strategy_greeks(
            spot=market.spot_price,
            strategy=strategy,
            market=market,
            time_years=config.simulation.num_days / ContractConstants.TRADING_DAYS_PER_YEAR,
        )

        # Initialize hedging
        hedge_shares = 0
        total_hedge_cost = 0.0
        num_rehedges = 0
        last_hedge_day = -999

        if hedging.is_enabled:
            hedge_shares = int(round(-entry_delta * ContractConstants.SHARES_PER_CONTRACT))
            num_rehedges = 1

        # Track P&L through path
        daily_pnls = []
        max_gain = 0.0
        max_loss = 0.0

        # Simulate each day
        num_days = len(path) - 1
        for day in range(num_days + 1):
            spot = path[day]
            time_elapsed = day / ContractConstants.TRADING_DAYS_PER_YEAR
            time_remaining = max(0, config.simulation.num_days / ContractConstants.TRADING_DAYS_PER_YEAR - time_elapsed)

            # Get IV for this day (may change in IV_CRUSH/IV_INCREASES scenarios)
            time_progress = day / num_days if num_days > 0 else 0
            current_iv = scenario.get_iv_at_time(market.entry_iv, time_progress)

            # Calculate current strategy value
            current_price = self._calculate_strategy_price(
                spot=spot,
                strategy=strategy,
                market=market,
                time_years=time_remaining,
                iv_override=current_iv,
            )

            # P&L (option prices are per-share, multiply by 100 for contract value)
            option_pnl = (current_price - entry_price) * ContractConstants.SHARES_PER_CONTRACT
            position_pnl = option_pnl

            # Add hedge P&L if hedging (hedge_shares is already in shares)
            if hedging.is_enabled and day > 0:
                spot_change = path[day] - path[day - 1]
                hedge_pnl = hedge_shares * spot_change  # Total hedge P&L in dollars
                position_pnl += hedge_pnl

                # Check if should rehedge
                should_rehedge = False
                if hedging.mode == HedgingMode.DAILY:
                    should_rehedge = True
                elif hedging.mode == HedgingMode.WEEKLY:
                    should_rehedge = (day - last_hedge_day) >= hedging.frequency
                elif hedging.mode == HedgingMode.THRESHOLD:
                    current_delta, _, _, _ = self._calculate_strategy_greeks(
                        spot=spot,
                        strategy=strategy,
                        market=market,
                        time_years=time_remaining,
                        iv_override=current_iv,
                    )
                    delta_drift = abs(current_delta - (-hedge_shares / ContractConstants.SHARES_PER_CONTRACT))
                    should_rehedge = delta_drift > hedging.threshold

                if should_rehedge and time_remaining > 0:
                    current_delta, _, _, _ = self._calculate_strategy_greeks(
                        spot=spot,
                        strategy=strategy,
                        market=market,
                        time_years=time_remaining,
                        iv_override=current_iv,
                    )
                    new_hedge_shares = int(round(-current_delta * ContractConstants.SHARES_PER_CONTRACT))
                    adjustment = new_hedge_shares - hedge_shares
                    total_hedge_cost += abs(adjustment) * spot * 0.0001  # Small transaction cost
                    hedge_shares = new_hedge_shares
                    num_rehedges += 1
                    last_hedge_day = day

            daily_pnls.append(position_pnl)
            max_gain = max(max_gain, position_pnl)
            max_loss = min(max_loss, position_pnl)

        # Calculate realized volatility
        returns = np.diff(np.log(path))
        realized_vol = float(np.std(returns) * np.sqrt(ContractConstants.TRADING_DAYS_PER_YEAR))

        # Final metrics
        final_pnl = daily_pnls[-1]
        entry_value = entry_price * ContractConstants.SHARES_PER_CONTRACT  # Total entry cost
        final_pnl_pct = (final_pnl / abs(entry_value)) * 100 if entry_value != 0 else 0

        # Scale Greeks to contract terms (× 100)
        contract_multiplier = ContractConstants.SHARES_PER_CONTRACT

        return SingleSimResult(
            path_id=path_id,
            scenario_name=scenario.name,
            hedging_mode=hedging.mode.value,
            strategy_name=strategy.name,
            final_pnl=final_pnl,
            final_pnl_pct=final_pnl_pct,
            max_gain=max_gain,
            max_loss=max_loss,
            final_spot=path[-1],
            spot_return=(path[-1] / path[0] - 1) * 100,
            realized_vol=realized_vol,
            num_rehedges=num_rehedges,
            total_hedge_cost=total_hedge_cost,
            entry_delta=entry_delta * contract_multiplier,
            entry_gamma=entry_gamma * contract_multiplier,
            entry_vega=entry_vega * contract_multiplier,
            entry_theta=entry_theta * contract_multiplier,
        )

    def _calculate_strategy_price(
        self,
        spot: float,
        strategy: StrategyConfig,
        market: MarketConfig,
        time_years: float,
        iv_override: Optional[float] = None,
    ) -> float:
        """Calculate total strategy price."""
        total = 0.0
        iv = iv_override if iv_override is not None else market.entry_iv

        for leg in strategy.legs:
            strike = leg.get_strike(market.spot_price)
            is_call = leg.option_type == OptionType.CALL

            leg_price = bs_price(
                S=spot,
                K=strike,
                T=time_years,
                r=market.risk_free_rate,
                sigma=iv,
                is_call=is_call,
            )

            total += leg_price * leg.position_sign * leg.quantity

        return total

    def _calculate_strategy_greeks(
        self,
        spot: float,
        strategy: StrategyConfig,
        market: MarketConfig,
        time_years: float,
        iv_override: Optional[float] = None,
    ) -> Tuple[float, float, float, float]:
        """Calculate total strategy Greeks (delta, gamma, vega, theta)."""
        total_delta = 0.0
        total_gamma = 0.0
        total_vega = 0.0
        total_theta = 0.0
        iv = iv_override if iv_override is not None else market.entry_iv

        for leg in strategy.legs:
            strike = leg.get_strike(market.spot_price)
            is_call = leg.option_type == OptionType.CALL
            multiplier = leg.position_sign * leg.quantity

            total_delta += bs_delta(spot, strike, time_years, market.risk_free_rate, iv, is_call) * multiplier
            total_gamma += bs_gamma(spot, strike, time_years, market.risk_free_rate, iv) * multiplier
            total_vega += bs_vega(spot, strike, time_years, market.risk_free_rate, iv) * multiplier
            total_theta += bs_theta(spot, strike, time_years, market.risk_free_rate, iv, is_call) * multiplier

        return total_delta, total_gamma, total_vega, total_theta

    def _aggregate_results(
        self,
        single_results: List[SingleSimResult],
        config: SimulationConfig,
        scenario: ScenarioConfig,
        hedging: HedgingConfig,
    ) -> AggregatedResults:
        """Aggregate single simulation results."""
        pnls = np.array([r.final_pnl for r in single_results])
        pnl_pcts = np.array([r.final_pnl_pct for r in single_results])

        agg = AggregatedResults(
            config_name=config.strategy.name,
            scenario_name=scenario.name,
            hedging_mode=hedging.mode.value,
            strategy_name=config.strategy.name,
            num_simulations=len(single_results),
            pnls=pnls,
            pnl_pcts=pnl_pcts,
            avg_entry_delta=np.mean([r.entry_delta for r in single_results]),
            avg_entry_gamma=np.mean([r.entry_gamma for r in single_results]),
            avg_rehedges=np.mean([r.num_rehedges for r in single_results]),
        )

        return agg


# ============================================================================
# CONVENIENCE FUNCTIONS
# ============================================================================

def run_quick_simulation(
    strategy: str = "long_call",
    num_sims: int = 1000,
    scenarios: str = "all",
    hedging: str = "both",
    spot: float = 100.0,
    iv: float = 0.25,
    days: int = 30,
    seed: Optional[int] = None,
    progress: bool = True,
) -> List[AggregatedResults]:
    """
    Quick simulation runner for common setups.

    Args:
        strategy: "long_call", "short_call", "straddle", etc.
        num_sims: Number of Monte Carlo simulations
        scenarios: "all", "iv_equals_rv", "iv_greater_rv", "iv_less_rv"
        hedging: "none", "daily", "both", "all"
        spot: Initial spot price
        iv: Entry implied volatility
        days: Days to expiration
        seed: Random seed for reproducibility
        progress: Show progress bar

    Returns:
        List of AggregatedResults

    Example:
        results = run_quick_simulation(
            strategy="long_call",
            num_sims=1000,
            scenarios="all",
            hedging="both",
        )

        for r in results:
            print(r.summary())
    """
    from config import quick_config

    config = quick_config(
        strategy=strategy,
        spot=spot,
        iv=iv,
        days=days,
        num_sims=num_sims,
        scenarios=scenarios,
        hedging=hedging,
        seed=seed,
    )

    engine = SimulationEngine(progress_bar=progress)
    return engine.run(config)
