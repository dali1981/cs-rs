#!/usr/bin/env python3
"""
Black-Scholes option pricing and Greeks calculation.

Provides:
- European option pricing (calls and puts)
- Greeks: delta, gamma, vega, theta, rho
- Implied volatility solver
"""

import numpy as np
from scipy.stats import norm
from scipy.optimize import brentq
from enum import Enum


class OptionType(Enum):
    """Option type."""
    CALL = 1
    PUT = -1


class BlackScholes:
    """Black-Scholes option pricing model."""

    @staticmethod
    def d1(S: float, K: float, T: float, r: float, sigma: float) -> float:
        """Compute d1 in Black-Scholes formula."""
        if T <= 0 or sigma <= 0:
            return np.inf if S > K else -np.inf
        return (np.log(S / K) + (r + 0.5 * sigma ** 2) * T) / (sigma * np.sqrt(T))

    @staticmethod
    def d2(S: float, K: float, T: float, r: float, sigma: float) -> float:
        """Compute d2 in Black-Scholes formula."""
        d1 = BlackScholes.d1(S, K, T, r, sigma)
        if d1 == np.inf:
            return np.inf
        if d1 == -np.inf:
            return -np.inf
        return d1 - sigma * np.sqrt(T)

    @staticmethod
    def price(
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
        option_type: OptionType,
    ) -> float:
        """
        Calculate European option price using Black-Scholes.

        Parameters:
        -----------
        S : float
            Current spot price
        K : float
            Strike price
        T : float
            Time to expiration in years
        r : float
            Risk-free rate (annualized)
        sigma : float
            Volatility (annualized)
        option_type : OptionType
            Call or Put

        Returns:
        --------
        float
            Option price
        """
        if T <= 0:
            # Intrinsic value
            if option_type == OptionType.CALL:
                return max(S - K, 0)
            else:
                return max(K - S, 0)

        if sigma <= 0:
            # No volatility - intrinsic value
            if option_type == OptionType.CALL:
                return max(S - K * np.exp(-r * T), 0)
            else:
                return max(K * np.exp(-r * T) - S, 0)

        d1 = BlackScholes.d1(S, K, T, r, sigma)
        d2 = BlackScholes.d2(S, K, T, r, sigma)

        if option_type == OptionType.CALL:
            return S * norm.cdf(d1) - K * np.exp(-r * T) * norm.cdf(d2)
        else:  # PUT
            return K * np.exp(-r * T) * norm.cdf(-d2) - S * norm.cdf(-d1)

    @staticmethod
    def delta(
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
        option_type: OptionType,
    ) -> float:
        """
        Calculate option delta (dPrice/dSpot).

        Delta represents:
        - For calls: sensitivity to stock price increase
        - For puts: negative sensitivity
        - Hedge ratio: how many shares to hold to delta-hedge
        """
        if T <= 0:
            if option_type == OptionType.CALL:
                return 1.0 if S > K else 0.0
            else:
                return -1.0 if S < K else 0.0

        if sigma <= 0:
            if option_type == OptionType.CALL:
                return 1.0 if S > K else 0.0
            else:
                return -1.0 if S < K else 0.0

        d1 = BlackScholes.d1(S, K, T, r, sigma)
        return norm.cdf(d1) if option_type == OptionType.CALL else norm.cdf(d1) - 1

    @staticmethod
    def gamma(
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
    ) -> float:
        """
        Calculate option gamma (d²Price/dSpot²).

        Gamma represents:
        - Convexity of option value
        - How much delta changes when spot moves
        - Always positive for long options
        """
        if T <= 0 or sigma <= 0:
            return 0.0

        d1 = BlackScholes.d1(S, K, T, r, sigma)
        return norm.pdf(d1) / (S * sigma * np.sqrt(T))

    @staticmethod
    def vega(
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
    ) -> float:
        """
        Calculate option vega (dPrice/dVolatility).

        Vega represents:
        - Sensitivity to volatility changes (per 1% change)
        - How much option value changes if IV changes
        - Always positive for long options
        """
        if T <= 0 or sigma <= 0:
            return 0.0

        d1 = BlackScholes.d1(S, K, T, r, sigma)
        return S * norm.pdf(d1) * np.sqrt(T) / 100  # Per 1% volatility change

    @staticmethod
    def theta(
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
        option_type: OptionType,
    ) -> float:
        """
        Calculate option theta (dPrice/dTime).

        Theta represents:
        - Time decay (per day)
        - How much option value decays per trading day
        - Usually positive for short options, negative for long
        """
        if T <= 0:
            return 0.0

        if sigma <= 0:
            return 0.0

        d1 = BlackScholes.d1(S, K, T, r, sigma)
        d2 = BlackScholes.d2(S, K, T, r, sigma)
        sqrt_T = np.sqrt(T)

        if option_type == OptionType.CALL:
            theta = (
                -S * norm.pdf(d1) * sigma / (2 * sqrt_T)
                - r * K * np.exp(-r * T) * norm.cdf(d2)
            )
        else:  # PUT
            theta = (
                -S * norm.pdf(d1) * sigma / (2 * sqrt_T)
                + r * K * np.exp(-r * T) * norm.cdf(-d2)
            )

        # Convert to daily theta (divide by 365)
        return theta / 365

    @staticmethod
    def rho(
        S: float,
        K: float,
        T: float,
        r: float,
        sigma: float,
        option_type: OptionType,
    ) -> float:
        """
        Calculate option rho (dPrice/dRate).

        Rho represents sensitivity to interest rate changes.
        Usually not significant for short-dated options.
        """
        if T <= 0 or sigma <= 0:
            return 0.0

        d2 = BlackScholes.d2(S, K, T, r, sigma)

        if option_type == OptionType.CALL:
            return K * T * np.exp(-r * T) * norm.cdf(d2)
        else:  # PUT
            return -K * T * np.exp(-r * T) * norm.cdf(-d2)

    @staticmethod
    def implied_volatility(
        option_price: float,
        S: float,
        K: float,
        T: float,
        r: float,
        option_type: OptionType,
        initial_guess: float = 0.3,
    ) -> float:
        """
        Solve for implied volatility using Brent's method.

        Parameters:
        -----------
        option_price : float
            Observed option price
        S : float
            Current spot price
        K : float
            Strike price
        T : float
            Time to expiration in years
        r : float
            Risk-free rate
        option_type : OptionType
            Call or Put
        initial_guess : float
            Initial volatility guess (default 0.3)

        Returns:
        --------
        float
            Implied volatility
        """
        def objective(sigma):
            return (
                BlackScholes.price(S, K, T, r, sigma, option_type) - option_price
            )

        try:
            # Search between 0.001 and 2.0 (0.1% to 200%)
            iv = brentq(objective, 0.001, 2.0, maxiter=100)
            return iv
        except ValueError:
            # If no solution found in range, return the initial guess
            return initial_guess


class GreeksSummary:
    """Summary of all Greeks for a position."""

    def __init__(
        self,
        price: float,
        delta: float,
        gamma: float,
        vega: float,
        theta: float,
        rho: float,
    ):
        self.price = price
        self.delta = delta
        self.gamma = gamma
        self.vega = vega
        self.theta = theta
        self.rho = rho

    def __repr__(self):
        return (
            f"Greeks(price={self.price:.2f}, delta={self.delta:.4f}, "
            f"gamma={self.gamma:.6f}, vega={self.vega:.4f}, "
            f"theta={self.theta:.4f}, rho={self.rho:.4f})"
        )


def calculate_greeks(
    S: float,
    K: float,
    T: float,
    r: float,
    sigma: float,
    option_type: OptionType,
) -> GreeksSummary:
    """Calculate all Greeks for an option position."""
    price = BlackScholes.price(S, K, T, r, sigma, option_type)
    delta = BlackScholes.delta(S, K, T, r, sigma, option_type)
    gamma = BlackScholes.gamma(S, K, T, r, sigma)
    vega = BlackScholes.vega(S, K, T, r, sigma)
    theta = BlackScholes.theta(S, K, T, r, sigma, option_type)
    rho = BlackScholes.rho(S, K, T, r, sigma, option_type)

    return GreeksSummary(
        price=price,
        delta=delta,
        gamma=gamma,
        vega=vega,
        theta=theta,
        rho=rho,
    )
