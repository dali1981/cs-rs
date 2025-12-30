use statrs::distribution::{ContinuousCDF, Normal};
use thiserror::Error;

use crate::greeks::Greeks;

#[derive(Error, Debug)]
pub enum BSError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("IV solver failed to converge")]
    ConvergenceFailure,
}

/// Configuration for Black-Scholes calculations
#[derive(Debug, Clone, Copy)]
pub struct BSConfig {
    pub risk_free_rate: f64,
    pub min_iv: f64,
    pub max_iv: f64,
    pub tolerance: f64,
    pub max_iterations: usize,
}

impl Default for BSConfig {
    fn default() -> Self {
        Self {
            risk_free_rate: 0.05,
            min_iv: 0.0001,
            max_iv: 5.0,
            tolerance: 1e-6,
            max_iterations: 100,
        }
    }
}

/// Calculate option price using Black-Scholes formula
#[inline]
pub fn bs_price(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> f64 {
    if time_to_expiry <= 0.0 || volatility <= 0.0 {
        return if is_call {
            (spot - strike).max(0.0)
        } else {
            (strike - spot).max(0.0)
        };
    }

    let sqrt_t = time_to_expiry.sqrt();
    let d1 = ((spot / strike).ln() + (risk_free_rate + 0.5 * volatility.powi(2)) * time_to_expiry)
        / (volatility * sqrt_t);
    let d2 = d1 - volatility * sqrt_t;

    let norm = Normal::new(0.0, 1.0).unwrap();
    let discount = (-risk_free_rate * time_to_expiry).exp();

    if is_call {
        spot * norm.cdf(d1) - strike * discount * norm.cdf(d2)
    } else {
        strike * discount * norm.cdf(-d2) - spot * norm.cdf(-d1)
    }
}

/// Calculate implied volatility using Brent's method
pub fn bs_implied_volatility(
    option_price: f64,
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    is_call: bool,
    config: &BSConfig,
) -> Option<f64> {
    if option_price <= 0.0 || spot <= 0.0 || strike <= 0.0 || time_to_expiry <= 0.0 {
        return None;
    }

    // Check arbitrage bounds
    let discount = (-config.risk_free_rate * time_to_expiry).exp();
    let (intrinsic, max_price) = if is_call {
        ((spot - strike * discount).max(0.0), spot)
    } else {
        ((strike * discount - spot).max(0.0), strike * discount)
    };

    if option_price < intrinsic || option_price > max_price {
        return None;
    }

    // Objective function for root finding
    let objective = |sigma: f64| -> f64 {
        bs_price(spot, strike, time_to_expiry, sigma, is_call, config.risk_free_rate) - option_price
    };

    // Brent's method
    let mut tolerance = config.tolerance;
    match roots::find_root_brent(config.min_iv, config.max_iv, objective, &mut tolerance) {
        Ok(iv) => Some(iv),
        Err(_) => None,
    }
}

/// Calculate Black-Scholes delta
///
/// Returns:
/// - Call: N(d1) ∈ [0, 1]
/// - Put: N(d1) - 1 ∈ [-1, 0]
#[inline]
pub fn bs_delta(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> f64 {
    if time_to_expiry <= 0.0 || volatility <= 0.0 {
        // At expiry
        return if is_call {
            if spot > strike { 1.0 } else { 0.0 }
        } else {
            if spot < strike { -1.0 } else { 0.0 }
        };
    }

    let sqrt_t = time_to_expiry.sqrt();
    let d1 = ((spot / strike).ln() + (risk_free_rate + 0.5 * volatility.powi(2)) * time_to_expiry)
        / (volatility * sqrt_t);

    let norm = Normal::new(0.0, 1.0).unwrap();

    if is_call {
        norm.cdf(d1)
    } else {
        norm.cdf(d1) - 1.0
    }
}

/// Calculate all Greeks efficiently in one pass
pub fn bs_greeks(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> Greeks {
    if time_to_expiry <= 0.0 || volatility <= 0.0 {
        return Greeks::at_expiry(spot, strike, is_call);
    }

    let sqrt_t = time_to_expiry.sqrt();
    let d1 = ((spot / strike).ln() + (risk_free_rate + 0.5 * volatility.powi(2)) * time_to_expiry)
        / (volatility * sqrt_t);
    let d2 = d1 - volatility * sqrt_t;

    let norm = Normal::new(0.0, 1.0).unwrap();
    let n_d1 = norm.cdf(d1);
    let n_d2 = norm.cdf(d2);
    let n_prime_d1 = (-0.5 * d1.powi(2)).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let discount = (-risk_free_rate * time_to_expiry).exp();

    let delta = if is_call { n_d1 } else { n_d1 - 1.0 };
    let gamma = n_prime_d1 / (spot * volatility * sqrt_t);
    let vega = spot * n_prime_d1 * sqrt_t * 0.01; // Per 1% vol change

    let theta = if is_call {
        (-spot * n_prime_d1 * volatility / (2.0 * sqrt_t)
            - risk_free_rate * strike * discount * n_d2)
            / 365.0
    } else {
        (-spot * n_prime_d1 * volatility / (2.0 * sqrt_t)
            + risk_free_rate * strike * discount * norm.cdf(-d2))
            / 365.0
    };

    let rho = if is_call {
        strike * time_to_expiry * discount * n_d2 * 0.01
    } else {
        -strike * time_to_expiry * discount * norm.cdf(-d2) * 0.01
    };

    Greeks { delta, gamma, theta, vega, rho }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_bs_price_call_atm() {
        let price = bs_price(100.0, 100.0, 1.0, 0.2, true, 0.05);
        // Known value from BS formula
        assert_relative_eq!(price, 10.45, epsilon = 0.01);
    }

    #[test]
    fn test_bs_price_put_atm() {
        let price = bs_price(100.0, 100.0, 1.0, 0.2, false, 0.05);
        // Put-call parity check
        let call_price = bs_price(100.0, 100.0, 1.0, 0.2, true, 0.05);
        let discount = (-0.05 * 1.0_f64).exp();
        assert_relative_eq!(
            call_price - price,
            100.0 - 100.0 * discount,
            epsilon = 0.01
        );
    }

    #[test]
    fn test_bs_price_at_expiry() {
        // Call ITM
        let price = bs_price(110.0, 100.0, 0.0, 0.2, true, 0.05);
        assert_relative_eq!(price, 10.0, epsilon = 1e-10);

        // Call OTM
        let price = bs_price(90.0, 100.0, 0.0, 0.2, true, 0.05);
        assert_relative_eq!(price, 0.0, epsilon = 1e-10);

        // Put ITM
        let price = bs_price(90.0, 100.0, 0.0, 0.2, false, 0.05);
        assert_relative_eq!(price, 10.0, epsilon = 1e-10);
    }

    #[test]
    fn test_bs_implied_volatility_roundtrip() {
        let spot = 100.0;
        let strike = 100.0;
        let ttm = 0.25;
        let vol = 0.30;
        let is_call = true;
        let config = BSConfig::default();

        let price = bs_price(spot, strike, ttm, vol, is_call, config.risk_free_rate);
        let iv = bs_implied_volatility(price, spot, strike, ttm, is_call, &config);

        assert!(iv.is_some());
        assert_relative_eq!(iv.unwrap(), vol, epsilon = 1e-4);
    }

    #[test]
    fn test_bs_implied_volatility_bounds() {
        let config = BSConfig::default();

        // Price below intrinsic
        let iv = bs_implied_volatility(0.5, 100.0, 95.0, 1.0, true, &config);
        assert!(iv.is_none());

        // Price above max
        let iv = bs_implied_volatility(105.0, 100.0, 100.0, 1.0, true, &config);
        assert!(iv.is_none());
    }

    #[test]
    fn test_bs_greeks_call_atm() {
        let greeks = bs_greeks(100.0, 100.0, 1.0, 0.2, true, 0.05);

        // ATM call delta should be around 0.5-0.6 (slightly above 0.5 due to drift)
        assert!(greeks.delta > 0.5 && greeks.delta < 0.65);

        // Gamma should be positive
        assert!(greeks.gamma > 0.0);

        // Vega should be positive
        assert!(greeks.vega > 0.0);

        // Theta should be negative
        assert!(greeks.theta < 0.0);
    }

    #[test]
    fn test_bs_greeks_put_atm() {
        let greeks = bs_greeks(100.0, 100.0, 1.0, 0.2, false, 0.05);

        // ATM put delta should be around -0.4 to -0.5
        assert!(greeks.delta < 0.0 && greeks.delta > -0.6);

        // Gamma should be positive (same as call)
        assert!(greeks.gamma > 0.0);
    }

    #[test]
    fn test_bs_greeks_at_expiry() {
        // ITM call at expiry
        let greeks = bs_greeks(110.0, 100.0, 0.0, 0.2, true, 0.05);
        assert_relative_eq!(greeks.delta, 1.0, epsilon = 1e-10);
        assert_relative_eq!(greeks.gamma, 0.0, epsilon = 1e-10);

        // OTM call at expiry
        let greeks = bs_greeks(90.0, 100.0, 0.0, 0.2, true, 0.05);
        assert_relative_eq!(greeks.delta, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_bs_delta_call_atm() {
        // ATM call delta should be around 0.5-0.6
        let delta = bs_delta(100.0, 100.0, 1.0, 0.2, true, 0.05);
        assert!(delta > 0.5 && delta < 0.65);
    }

    #[test]
    fn test_bs_delta_put_atm() {
        // ATM put delta should be around -0.4 to -0.5
        let delta = bs_delta(100.0, 100.0, 1.0, 0.2, false, 0.05);
        assert!(delta > -0.5 && delta < -0.35);
    }

    #[test]
    fn test_bs_delta_call_put_parity() {
        // call_delta - put_delta = 1
        let call_delta = bs_delta(100.0, 100.0, 1.0, 0.2, true, 0.05);
        let put_delta = bs_delta(100.0, 100.0, 1.0, 0.2, false, 0.05);
        assert_relative_eq!(call_delta - put_delta, 1.0, epsilon = 1e-6);
    }

    #[test]
    fn test_bs_delta_at_expiry() {
        // ITM call at expiry has delta 1
        let delta = bs_delta(110.0, 100.0, 0.0, 0.2, true, 0.05);
        assert_relative_eq!(delta, 1.0, epsilon = 1e-10);

        // OTM call at expiry has delta 0
        let delta = bs_delta(90.0, 100.0, 0.0, 0.2, true, 0.05);
        assert_relative_eq!(delta, 0.0, epsilon = 1e-10);

        // ITM put at expiry has delta -1
        let delta = bs_delta(90.0, 100.0, 0.0, 0.2, false, 0.05);
        assert_relative_eq!(delta, -1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_bs_greeks_put_call_parity() {
        let spot = 100.0;
        let strike = 100.0;
        let ttm = 1.0;
        let vol = 0.2;
        let r = 0.05;

        let call_greeks = bs_greeks(spot, strike, ttm, vol, true, r);
        let put_greeks = bs_greeks(spot, strike, ttm, vol, false, r);

        // Delta: call_delta - put_delta = 1
        assert_relative_eq!(call_greeks.delta - put_greeks.delta, 1.0, epsilon = 1e-6);

        // Gamma should be the same
        assert_relative_eq!(call_greeks.gamma, put_greeks.gamma, epsilon = 1e-6);

        // Vega should be the same
        assert_relative_eq!(call_greeks.vega, put_greeks.vega, epsilon = 1e-6);
    }
}
