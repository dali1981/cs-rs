#!/usr/bin/env python3
"""
Compare option pricing extrapolation methods for missing strikes.

Example: CRBG strike 32 call (Dec 19, 2025)
- Available strikes: 34.0 ($0.70), 35.0 ($0.38)
- Target strike: 32.0 (missing)
- Spot: $32.00
"""

import numpy as np
from scipy.stats import norm
from datetime import datetime, timedelta
from typing import Optional, Tuple
import pandas as pd


class BlackScholes:
    """Black-Scholes option pricing and implied volatility."""

    @staticmethod
    def price(S: float, K: float, T: float, sigma: float, r: float = 0.0, is_call: bool = True) -> float:
        """Calculate option price using Black-Scholes formula.

        Args:
            S: Spot price
            K: Strike price
            T: Time to maturity (years)
            sigma: Implied volatility (annualized)
            r: Risk-free rate
            is_call: True for call, False for put

        Returns:
            Option price
        """
        if T <= 0:
            return max(S - K, 0) if is_call else max(K - S, 0)

        d1 = (np.log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * np.sqrt(T))
        d2 = d1 - sigma * np.sqrt(T)

        if is_call:
            return S * norm.cdf(d1) - K * np.exp(-r * T) * norm.cdf(d2)
        else:
            return K * np.exp(-r * T) * norm.cdf(-d2) - S * norm.cdf(-d1)

    @staticmethod
    def implied_vol(price: float, S: float, K: float, T: float, r: float = 0.0,
                   is_call: bool = True, max_iter: int = 100, tol: float = 1e-6) -> Optional[float]:
        """Calculate implied volatility using Newton-Raphson method.

        Args:
            price: Market option price
            S: Spot price
            K: Strike price
            T: Time to maturity (years)
            r: Risk-free rate
            is_call: True for call, False for put
            max_iter: Maximum iterations
            tol: Tolerance for convergence

        Returns:
            Implied volatility or None if convergence fails
        """
        # Initial guess
        sigma = 0.3

        for i in range(max_iter):
            # Calculate price and vega
            calc_price = BlackScholes.price(S, K, T, sigma, r, is_call)

            if abs(calc_price - price) < tol:
                return sigma

            # Vega (derivative of price w.r.t. volatility)
            d1 = (np.log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * np.sqrt(T))
            vega = S * norm.pdf(d1) * np.sqrt(T)

            if vega < 1e-10:
                return None

            # Newton-Raphson step
            sigma = sigma - (calc_price - price) / vega

            # Keep sigma positive
            if sigma <= 0:
                sigma = 0.01

        return None

    @staticmethod
    def greeks(S: float, K: float, T: float, sigma: float, r: float = 0.0, is_call: bool = True) -> dict:
        """Calculate option Greeks.

        Returns:
            Dictionary with delta, gamma, theta, vega
        """
        if T <= 0:
            return {'delta': 1.0 if (is_call and S > K) else 0.0,
                   'gamma': 0.0, 'theta': 0.0, 'vega': 0.0}

        d1 = (np.log(S / K) + (r + 0.5 * sigma**2) * T) / (sigma * np.sqrt(T))
        d2 = d1 - sigma * np.sqrt(T)

        if is_call:
            delta = norm.cdf(d1)
            theta = (-S * norm.pdf(d1) * sigma / (2 * np.sqrt(T))
                    - r * K * np.exp(-r * T) * norm.cdf(d2))
        else:
            delta = -norm.cdf(-d1)
            theta = (-S * norm.pdf(d1) * sigma / (2 * np.sqrt(T))
                    + r * K * np.exp(-r * T) * norm.cdf(-d2))

        gamma = norm.pdf(d1) / (S * sigma * np.sqrt(T))
        vega = S * norm.pdf(d1) * np.sqrt(T)

        return {'delta': delta, 'gamma': gamma, 'theta': theta / 365, 'vega': vega / 100}


class OptionChainData:
    """Mock option chain data for CRBG."""

    def __init__(self):
        self.spot = 32.0
        self.pricing_date = datetime(2025, 11, 3)
        self.expiration = datetime(2025, 12, 19)

        # Available strikes with market prices
        self.strikes = {
            34.0: {'price': 0.70, 'volume': 1},
            35.0: {'price': 0.38, 'volume': 13},
        }

        # Target strike (missing from market)
        self.target_strike = 32.0

    def ttm_years(self) -> float:
        """Calculate time to maturity in years."""
        days = (self.expiration - self.pricing_date).days
        return days / 365.0

    def get_nearest_strike(self, target: float, direction: str = 'closest') -> Optional[Tuple[float, dict]]:
        """Find nearest strike in specified direction.

        Args:
            target: Target strike
            direction: 'closest', 'above', 'below'

        Returns:
            (strike, data) tuple or None
        """
        available = sorted(self.strikes.keys())

        if direction == 'closest':
            nearest = min(available, key=lambda k: abs(k - target))
            return (nearest, self.strikes[nearest])
        elif direction == 'above':
            above = [k for k in available if k > target]
            if above:
                return (min(above), self.strikes[min(above)])
        elif direction == 'below':
            below = [k for k in available if k < target]
            if below:
                return (max(below), self.strikes[max(below)])

        return None


class FlatIVExtrapolation:
    """Method 1: Use nearest strike's IV, price with Black-Scholes."""

    @staticmethod
    def price(chain: OptionChainData, target_strike: float) -> dict:
        """Price target strike using flat IV extrapolation.

        Strategy:
        1. Find nearest available strike
        2. Back out its implied volatility
        3. Use that IV to price target strike with Black-Scholes

        Returns:
            Dictionary with price, iv, method, details
        """
        # Find nearest strike (prefer above for calls)
        nearest_result = chain.get_nearest_strike(target_strike, 'above')
        if not nearest_result:
            nearest_result = chain.get_nearest_strike(target_strike, 'closest')

        if not nearest_result:
            return {'price': None, 'iv': None, 'method': 'flat_iv', 'error': 'No strikes available'}

        nearest_strike, nearest_data = nearest_result
        nearest_price = nearest_data['price']

        # Calculate implied volatility from nearest strike
        ttm = chain.ttm_years()
        iv = BlackScholes.implied_vol(
            price=nearest_price,
            S=chain.spot,
            K=nearest_strike,
            T=ttm,
            is_call=True
        )

        if iv is None:
            return {'price': None, 'iv': None, 'method': 'flat_iv', 'error': 'IV calculation failed'}

        # Price target strike using this IV
        target_price = BlackScholes.price(
            S=chain.spot,
            K=target_strike,
            T=ttm,
            sigma=iv,
            is_call=True
        )

        # Calculate Greeks
        greeks = BlackScholes.greeks(chain.spot, target_strike, ttm, iv, is_call=True)

        return {
            'price': target_price,
            'iv': iv,
            'method': 'flat_iv',
            'nearest_strike': nearest_strike,
            'nearest_price': nearest_price,
            'greeks': greeks,
            'details': f"Used IV={iv:.4f} from strike {nearest_strike} (${nearest_price:.2f})"
        }


class IntrinsicValueAdjustment:
    """Method 2: Adjust nearest strike's price by intrinsic value difference."""

    @staticmethod
    def price(chain: OptionChainData, target_strike: float) -> dict:
        """Price target strike using intrinsic value adjustment.

        Strategy:
        1. Find nearest available strike
        2. Calculate intrinsic value difference
        3. Adjust price: target_price = nearest_price + (target_intrinsic - nearest_intrinsic)

        For calls: Intrinsic = max(S - K, 0)

        Returns:
            Dictionary with price, method, details
        """
        # Find nearest strike (prefer above for calls)
        nearest_result = chain.get_nearest_strike(target_strike, 'above')
        if not nearest_result:
            nearest_result = chain.get_nearest_strike(target_strike, 'closest')

        if not nearest_result:
            return {'price': None, 'method': 'intrinsic', 'error': 'No strikes available'}

        nearest_strike, nearest_data = nearest_result
        nearest_price = nearest_data['price']

        # Calculate intrinsic values
        target_intrinsic = max(chain.spot - target_strike, 0)
        nearest_intrinsic = max(chain.spot - nearest_strike, 0)

        # Adjust price
        intrinsic_diff = target_intrinsic - nearest_intrinsic
        target_price = nearest_price + intrinsic_diff

        # Calculate time value (for comparison)
        target_time_value = target_price - target_intrinsic
        nearest_time_value = nearest_price - nearest_intrinsic

        return {
            'price': target_price,
            'method': 'intrinsic',
            'nearest_strike': nearest_strike,
            'nearest_price': nearest_price,
            'target_intrinsic': target_intrinsic,
            'nearest_intrinsic': nearest_intrinsic,
            'intrinsic_diff': intrinsic_diff,
            'target_time_value': target_time_value,
            'nearest_time_value': nearest_time_value,
            'details': f"Adjusted ${nearest_price:.2f} by intrinsic diff ${intrinsic_diff:.2f}"
        }


def compare_methods(chain: OptionChainData):
    """Compare both extrapolation methods."""

    print("=" * 80)
    print("OPTION PRICING EXTRAPOLATION COMPARISON")
    print("=" * 80)
    print()
    print("Scenario: CRBG Calendar Straddle")
    print(f"  Spot Price:        ${chain.spot:.2f}")
    print(f"  Pricing Date:      {chain.pricing_date.strftime('%Y-%m-%d')}")
    print(f"  Expiration:        {chain.expiration.strftime('%Y-%m-%d')}")
    print(f"  Time to Maturity:  {chain.ttm_years():.4f} years ({(chain.expiration - chain.pricing_date).days} days)")
    print(f"  Target Strike:     ${chain.target_strike:.2f} (MISSING)")
    print()

    print("Available Market Data:")
    print(f"  {'Strike':<10} {'Price':<10} {'Volume':<10} {'Moneyness':<12}")
    print(f"  {'-'*10} {'-'*10} {'-'*10} {'-'*12}")
    for strike, data in sorted(chain.strikes.items()):
        moneyness = strike / chain.spot
        print(f"  ${strike:<9.2f} ${data['price']:<9.2f} {data['volume']:<10d} {moneyness:.4f}")
    print()

    # Calculate IVs for available strikes
    print("Implied Volatilities (from market prices):")
    ttm = chain.ttm_years()
    for strike, data in sorted(chain.strikes.items()):
        iv = BlackScholes.implied_vol(data['price'], chain.spot, strike, ttm, is_call=True)
        if iv:
            print(f"  Strike ${strike:.2f}: σ = {iv:.4f} ({iv*100:.2f}%)")
    print()

    print("=" * 80)
    print("METHOD 1: FLAT IV EXTRAPOLATION")
    print("=" * 80)
    result1 = FlatIVExtrapolation.price(chain, chain.target_strike)

    if result1['price'] is not None:
        print(f"  Nearest Strike:    ${result1['nearest_strike']:.2f} @ ${result1['nearest_price']:.2f}")
        print(f"  Implied Vol (σ):   {result1['iv']:.4f} ({result1['iv']*100:.2f}%)")
        print(f"  Target Price:      ${result1['price']:.4f}")
        print()
        print(f"  Greeks:")
        print(f"    Delta:  {result1['greeks']['delta']:>8.4f}")
        print(f"    Gamma:  {result1['greeks']['gamma']:>8.4f}")
        print(f"    Theta:  {result1['greeks']['theta']:>8.4f} (per day)")
        print(f"    Vega:   {result1['greeks']['vega']:>8.4f} (per 1% vol)")
        print()
        print(f"  Logic: {result1['details']}")
    else:
        print(f"  ERROR: {result1.get('error', 'Unknown error')}")

    print()
    print("=" * 80)
    print("METHOD 2: INTRINSIC VALUE ADJUSTMENT")
    print("=" * 80)
    result2 = IntrinsicValueAdjustment.price(chain, chain.target_strike)

    if result2['price'] is not None:
        print(f"  Nearest Strike:    ${result2['nearest_strike']:.2f} @ ${result2['nearest_price']:.2f}")
        print(f"  Target Intrinsic:  ${result2['target_intrinsic']:.4f}")
        print(f"  Nearest Intrinsic: ${result2['nearest_intrinsic']:.4f}")
        print(f"  Intrinsic Diff:    ${result2['intrinsic_diff']:.4f}")
        print(f"  Target Price:      ${result2['price']:.4f}")
        print()
        print(f"  Time Value Analysis:")
        print(f"    Target Time Value:  ${result2['target_time_value']:.4f}")
        print(f"    Nearest Time Value: ${result2['nearest_time_value']:.4f}")
        print(f"    Assumption: Time value constant across strikes")
        print()
        print(f"  Logic: {result2['details']}")
    else:
        print(f"  ERROR: {result2.get('error', 'Unknown error')}")

    print()
    print("=" * 80)
    print("COMPARISON")
    print("=" * 80)

    if result1['price'] is not None and result2['price'] is not None:
        diff = result1['price'] - result2['price']
        pct_diff = (diff / result2['price']) * 100 if result2['price'] != 0 else 0

        print(f"  Flat IV Price:         ${result1['price']:.4f}")
        print(f"  Intrinsic Adj Price:   ${result2['price']:.4f}")
        print(f"  Difference:            ${diff:.4f} ({pct_diff:+.2f}%)")
        print()

        # Determine which is more conservative
        if diff > 0:
            print(f"  → Intrinsic method is MORE CONSERVATIVE (lower price)")
        elif diff < 0:
            print(f"  → Flat IV method is MORE CONSERVATIVE (lower price)")
        else:
            print(f"  → Both methods agree")

        print()
        print("When to use each:")
        print("  • Flat IV: Better when volatility smile is relatively flat")
        print("  • Intrinsic: Better for ITM options or when IV data is sparse")
        print("  • Current case: Target is ATM, both methods should be similar")

    print()

    # Additional test: What if spot moves?
    print("=" * 80)
    print("SENSITIVITY ANALYSIS: ITM Scenario (Spot moves to $36)")
    print("=" * 80)

    chain_itm = OptionChainData()
    chain_itm.spot = 36.0  # Move spot up, making strikes 32, 34 ITM

    print(f"  New Spot: ${chain_itm.spot:.2f}")
    print(f"  Strike 32 is now ${chain_itm.spot - chain.target_strike:.2f} ITM")
    print(f"  Strike 34 is now ${chain_itm.spot - 34:.2f} ITM")
    print()

    result1_itm = FlatIVExtrapolation.price(chain_itm, chain.target_strike)
    result2_itm = IntrinsicValueAdjustment.price(chain_itm, chain.target_strike)

    if result1_itm.get('price') is not None:
        print(f"  Flat IV Price:         ${result1_itm['price']:.4f}")
    else:
        print(f"  Flat IV Price:         ERROR - {result1_itm.get('error', 'Unknown')}")

    if result2_itm.get('price') is not None:
        print(f"  Intrinsic Adj Price:   ${result2_itm['price']:.4f}")
    else:
        print(f"  Intrinsic Adj Price:   ERROR - {result2_itm.get('error', 'Unknown')}")

    if result1_itm.get('price') is not None and result2_itm.get('price') is not None:
        diff_itm = result1_itm['price'] - result2_itm['price']
        print(f"  Difference:            ${diff_itm:.4f}")
        print()
        print(f"  Intrinsic component for strike 32: ${max(chain_itm.spot - 32, 0):.2f}")
        print(f"  → Intrinsic method captures this value difference directly")
        print(f"  → Flat IV method relies on BS model to capture ITM value")


def main():
    """Run comparison analysis."""
    chain = OptionChainData()
    compare_methods(chain)

    # Create summary DataFrame
    results = []

    # ATM case (spot = 32)
    chain_atm = OptionChainData()
    r1 = FlatIVExtrapolation.price(chain_atm, chain_atm.target_strike)
    r2 = IntrinsicValueAdjustment.price(chain_atm, chain_atm.target_strike)

    results.append({
        'Scenario': 'ATM (S=32)',
        'Target Strike': 32.0,
        'Spot': 32.0,
        'Moneyness': 'ATM',
        'Flat IV Price': r1.get('price'),
        'Intrinsic Price': r2.get('price'),
        'Difference': r1.get('price', 0) - r2.get('price', 0) if r1.get('price') and r2.get('price') else None
    })

    # ITM case (spot = 36)
    chain_itm = OptionChainData()
    chain_itm.spot = 36.0
    r1 = FlatIVExtrapolation.price(chain_itm, chain_itm.target_strike)
    r2 = IntrinsicValueAdjustment.price(chain_itm, chain_itm.target_strike)

    results.append({
        'Scenario': 'ITM (S=36)',
        'Target Strike': 32.0,
        'Spot': 36.0,
        'Moneyness': f'{36-32:.0f} ITM',
        'Flat IV Price': r1.get('price'),
        'Intrinsic Price': r2.get('price'),
        'Difference': r1.get('price', 0) - r2.get('price', 0) if r1.get('price') and r2.get('price') else None
    })

    # OTM case (spot = 30)
    chain_otm = OptionChainData()
    chain_otm.spot = 30.0
    r1 = FlatIVExtrapolation.price(chain_otm, chain_otm.target_strike)
    r2 = IntrinsicValueAdjustment.price(chain_otm, chain_otm.target_strike)

    results.append({
        'Scenario': 'OTM (S=30)',
        'Target Strike': 32.0,
        'Spot': 30.0,
        'Moneyness': f'{32-30:.0f} OTM',
        'Flat IV Price': r1.get('price'),
        'Intrinsic Price': r2.get('price'),
        'Difference': r1.get('price', 0) - r2.get('price', 0) if r1.get('price') and r2.get('price') else None
    })

    print()
    print("=" * 80)
    print("SUMMARY TABLE")
    print("=" * 80)
    df = pd.DataFrame(results)
    print(df.to_string(index=False))
    print()


if __name__ == '__main__':
    main()
