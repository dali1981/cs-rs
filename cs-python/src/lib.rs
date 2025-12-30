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

#[pymodule]
fn cs_rust(_py: Python, m: &PyModule) -> PyResult<()> {
    // Analytics functions
    m.add_function(wrap_pyfunction!(py_bs_price, m)?)?;
    m.add_function(wrap_pyfunction!(py_bs_implied_volatility, m)?)?;
    m.add_function(wrap_pyfunction!(py_bs_greeks, m)?)?;

    // Domain types
    m.add_class::<PyGreeks>()?;

    // Backtest
    m.add_class::<PyBacktestConfig>()?;
    m.add_class::<PyBacktestResult>()?;
    m.add_class::<PyBacktestUseCase>()?;

    Ok(())
}
