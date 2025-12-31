// cs-python: PyO3 Python bindings
//
// Exposes Rust analytics and backtest engine to Python.

use pyo3::prelude::*;

mod analytics;
mod domain;
mod backtest;

use analytics::*;
use domain::*;
use backtest::*;

/// Calendar Spread Backtest - Rust Edition
///
/// High-performance backtest engine for calendar spread options strategies.
///
/// Modules:
///     - Analytics: Black-Scholes pricing, IV solver, Greeks calculation
///     - Domain: Trade results and configuration
///     - Backtest: Main backtest execution engine
///
/// Example:
///     >>> from cs_rust import PyBacktestConfig, PyBacktestUseCase
///     >>> config = PyBacktestConfig(data_dir="/path/to/data")
///     >>> backtest = PyBacktestUseCase(config)
///     >>> result = backtest.execute("2024-01-01", "2024-01-31", "call")
///     >>> print(f"Win rate: {result.win_rate():.2%}")
#[pymodule]
fn cs_rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Analytics functions
    m.add_function(wrap_pyfunction!(py_bs_price, m)?)?;
    m.add_function(wrap_pyfunction!(py_bs_implied_volatility, m)?)?;
    m.add_function(wrap_pyfunction!(py_bs_greeks, m)?)?;

    // Domain types
    m.add_class::<PyGreeks>()?;
    m.add_class::<PyCalendarSpreadResult>()?;

    // Backtest
    m.add_class::<PyBacktestConfig>()?;
    m.add_class::<PyBacktestResult>()?;
    m.add_class::<PyBacktestUseCase>()?;

    Ok(())
}
