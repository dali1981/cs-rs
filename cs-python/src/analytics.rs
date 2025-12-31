use pyo3::prelude::*;
use cs_analytics::{bs_price, bs_implied_volatility, bs_greeks, BSConfig};

/// Calculate option price using Black-Scholes formula
///
/// Args:
///     spot: Current spot price
///     strike: Strike price
///     time_to_expiry: Time to expiration in years
///     volatility: Implied volatility (as decimal, e.g., 0.20 for 20%)
///     is_call: True for call, False for put
///     risk_free_rate: Risk-free rate (as decimal, default: 0.05)
///
/// Returns:
///     Option price
#[pyfunction]
#[pyo3(signature = (spot, strike, time_to_expiry, volatility, is_call, risk_free_rate=0.05))]
pub fn py_bs_price(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> f64 {
    bs_price(spot, strike, time_to_expiry, volatility, is_call, risk_free_rate)
}

/// Calculate implied volatility from option price
///
/// Args:
///     option_price: Market price of the option
///     spot: Current spot price
///     strike: Strike price
///     time_to_expiry: Time to expiration in years
///     is_call: True for call, False for put
///
/// Returns:
///     Implied volatility (as decimal) or None if solver fails
#[pyfunction]
#[pyo3(signature = (option_price, spot, strike, time_to_expiry, is_call))]
pub fn py_bs_implied_volatility(
    option_price: f64,
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    is_call: bool,
) -> Option<f64> {
    bs_implied_volatility(option_price, spot, strike, time_to_expiry, is_call, &BSConfig::default())
}

/// Calculate option Greeks using Black-Scholes
///
/// Args:
///     spot: Current spot price
///     strike: Strike price
///     time_to_expiry: Time to expiration in years
///     volatility: Implied volatility (as decimal)
///     is_call: True for call, False for put
///     risk_free_rate: Risk-free rate (as decimal, default: 0.05)
///
/// Returns:
///     PyGreeks object with delta, gamma, theta, vega, rho
#[pyfunction]
#[pyo3(signature = (spot, strike, time_to_expiry, volatility, is_call, risk_free_rate=0.05))]
pub fn py_bs_greeks(
    spot: f64,
    strike: f64,
    time_to_expiry: f64,
    volatility: f64,
    is_call: bool,
    risk_free_rate: f64,
) -> PyGreeks {
    let greeks = bs_greeks(spot, strike, time_to_expiry, volatility, is_call, risk_free_rate);
    PyGreeks::from(greeks)
}

/// Option Greeks container
#[pyclass]
#[derive(Clone)]
pub struct PyGreeks {
    /// Delta: sensitivity to spot price change
    #[pyo3(get)]
    pub delta: f64,
    /// Gamma: rate of change of delta
    #[pyo3(get)]
    pub gamma: f64,
    /// Theta: time decay (per day)
    #[pyo3(get)]
    pub theta: f64,
    /// Vega: sensitivity to 1% volatility change
    #[pyo3(get)]
    pub vega: f64,
    /// Rho: sensitivity to 1% interest rate change
    #[pyo3(get)]
    pub rho: f64,
}

#[pymethods]
impl PyGreeks {
    fn __repr__(&self) -> String {
        format!(
            "PyGreeks(delta={:.4}, gamma={:.4}, theta={:.4}, vega={:.4}, rho={:.4})",
            self.delta, self.gamma, self.theta, self.vega, self.rho
        )
    }
}

impl From<cs_analytics::Greeks> for PyGreeks {
    fn from(g: cs_analytics::Greeks) -> Self {
        Self {
            delta: g.delta,
            gamma: g.gamma,
            theta: g.theta,
            vega: g.vega,
            rho: g.rho,
        }
    }
}
