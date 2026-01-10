use pyo3::prelude::*;
use std::path::PathBuf;
use chrono::NaiveDate;

use cs_backtest::{BacktestConfig, BacktestUseCase};
use cs_domain::{
    infrastructure::{ParquetEarningsRepository, FinqEquityRepository, FinqOptionsRepository},
    TimingConfig, TradeSelectionCriteria,
    ReturnBasis,
};
use finq_core::OptionType;

use crate::domain::PyCalendarSpreadResult;

/// Backtest configuration
#[pyclass]
#[derive(Clone)]
pub struct PyBacktestConfig {
    /// Data directory path
    #[pyo3(get, set)]
    pub data_dir: String,
    /// Entry hour (0-23)
    #[pyo3(get, set)]
    pub entry_hour: u32,
    /// Entry minute (0-59)
    #[pyo3(get, set)]
    pub entry_minute: u32,
    /// Exit hour (0-23)
    #[pyo3(get, set)]
    pub exit_hour: u32,
    /// Exit minute (0-59)
    #[pyo3(get, set)]
    pub exit_minute: u32,
    /// Minimum short DTE
    #[pyo3(get, set)]
    pub min_short_dte: i32,
    /// Maximum short DTE
    #[pyo3(get, set)]
    pub max_short_dte: i32,
    /// Minimum long DTE
    #[pyo3(get, set)]
    pub min_long_dte: i32,
    /// Maximum long DTE
    #[pyo3(get, set)]
    pub max_long_dte: i32,
    /// Target delta (optional)
    #[pyo3(get, set)]
    pub target_delta: Option<f64>,
    /// Minimum IV ratio (long/short, optional)
    #[pyo3(get, set)]
    pub min_iv_ratio: Option<f64>,
    /// Strategy type ("atm")
    #[pyo3(get, set)]
    pub strategy: String,
    /// Symbol filter (optional)
    #[pyo3(get, set)]
    pub symbols: Option<Vec<String>>,
    /// Minimum market cap filter (optional)
    #[pyo3(get, set)]
    pub min_market_cap: Option<u64>,
    /// Enable parallel processing
    #[pyo3(get, set)]
    pub parallel: bool,
    /// Wing width for iron butterfly (distance from ATM to wings)
    #[pyo3(get, set)]
    pub wing_width: f64,
}

#[pymethods]
impl PyBacktestConfig {
    #[new]
    #[pyo3(signature = (
        data_dir,
        entry_hour=9,
        entry_minute=35,
        exit_hour=15,
        exit_minute=55,
        min_short_dte=3,
        max_short_dte=45,
        min_long_dte=14,
        max_long_dte=90,
        target_delta=None,
        min_iv_ratio=None,
        strategy="atm".to_string(),
        symbols=None,
        min_market_cap=None,
        parallel=true,
        wing_width=10.0
    ))]
    fn new(
        data_dir: String,
        entry_hour: u32,
        entry_minute: u32,
        exit_hour: u32,
        exit_minute: u32,
        min_short_dte: i32,
        max_short_dte: i32,
        min_long_dte: i32,
        max_long_dte: i32,
        target_delta: Option<f64>,
        min_iv_ratio: Option<f64>,
        strategy: String,
        symbols: Option<Vec<String>>,
        min_market_cap: Option<u64>,
        parallel: bool,
        wing_width: f64,
    ) -> Self {
        Self {
            data_dir,
            entry_hour,
            entry_minute,
            exit_hour,
            exit_minute,
            min_short_dte,
            max_short_dte,
            min_long_dte,
            max_long_dte,
            target_delta,
            min_iv_ratio,
            strategy,
            symbols,
            min_market_cap,
            parallel,
            wing_width,
        }
    }

    fn __repr__(&self) -> String {
        format!("PyBacktestConfig(data_dir='{}', strategy='{}')", self.data_dir, self.strategy)
    }
}

/// Backtest result
#[pyclass]
#[derive(Clone)]
pub struct PyBacktestResult {
    /// All trade results
    #[pyo3(get)]
    pub results: Vec<PyCalendarSpreadResult>,
    /// Number of sessions processed
    #[pyo3(get)]
    pub sessions_processed: usize,
    /// Total number of entries
    #[pyo3(get)]
    pub total_entries: usize,
    /// Total opportunities (before filtering)
    #[pyo3(get)]
    pub total_opportunities: usize,
}

#[pymethods]
impl PyBacktestResult {
    /// Calculate win rate
    fn win_rate(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        let winners = self.results.iter().filter(|r| r.is_winner()).count();
        winners as f64 / self.results.len() as f64
    }

    /// Calculate total P&L
    fn total_pnl(&self) -> f64 {
        self.results.iter().map(|r| r.pnl).sum()
    }

    /// Calculate average P&L per trade
    fn avg_pnl(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        self.total_pnl() / self.results.len() as f64
    }

    fn __repr__(&self) -> String {
        format!(
            "PyBacktestResult(trades={}, win_rate={:.2}%, total_pnl=${:.2})",
            self.total_entries,
            self.win_rate() * 100.0,
            self.total_pnl()
        )
    }
}

/// Backtest use case - main entry point for running backtests
#[pyclass]
pub struct PyBacktestUseCase {
    config: PyBacktestConfig,
}

#[pymethods]
impl PyBacktestUseCase {
    #[new]
    fn new(config: PyBacktestConfig) -> Self {
        Self { config }
    }

    /// Run backtest
    ///
    /// Args:
    ///     start_date: Start date (YYYY-MM-DD)
    ///     end_date: End date (YYYY-MM-DD)
    ///     option_type: "call" or "put"
    ///
    /// Returns:
    ///     PyBacktestResult
    fn execute(
        &self,
        py: Python,
        start_date: String,
        end_date: String,
        option_type: String,
    ) -> PyResult<PyBacktestResult> {
        // Parse dates
        let start = NaiveDate::parse_from_str(&start_date, "%Y-%m-%d")
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("Invalid start date: {}", e)
            ))?;
        let end = NaiveDate::parse_from_str(&end_date, "%Y-%m-%d")
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(
                format!("Invalid end date: {}", e)
            ))?;

        // Parse option type
        let opt_type = match option_type.to_lowercase().as_str() {
            "call" => OptionType::Call,
            "put" => OptionType::Put,
            _ => return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "option_type must be 'call' or 'put'"
            )),
        };

        if self.config.strategy.to_lowercase() != "atm" {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "strategy must be 'atm'"
            ));
        }

        // Create Rust config
        let data_dir = PathBuf::from(&self.config.data_dir);
        let rust_config = BacktestConfig {
            data_dir: data_dir.clone(),
            earnings_dir: data_dir.clone(),  // Use same directory for earnings in Python
            earnings_file: None,
            timing: TimingConfig {
                entry_hour: self.config.entry_hour,
                entry_minute: self.config.entry_minute,
                exit_hour: self.config.exit_hour,
                exit_minute: self.config.exit_minute,
            },
            timing_strategy: None,
            entry_days_before: None,
            exit_days_before: None,
            entry_offset: None,
            holding_days: None,
            exit_days_after: None,
            selection: TradeSelectionCriteria {
                min_short_dte: self.config.min_short_dte,
                max_short_dte: self.config.max_short_dte,
                min_long_dte: self.config.min_long_dte,
                max_long_dte: self.config.max_long_dte,
                target_delta: self.config.target_delta,
                min_iv_ratio: self.config.min_iv_ratio,
                max_bid_ask_spread_pct: None,
            },
            spread: cs_backtest::SpreadType::Calendar,
            selection_strategy: cs_backtest::SelectionType::ATM,
            symbols: self.config.symbols.clone(),
            min_market_cap: self.config.min_market_cap,
            parallel: self.config.parallel,
            pricing_model: cs_analytics::PricingModel::default(),
            target_delta: self.config.target_delta.unwrap_or(0.50),
            delta_range: (0.25, 0.75),
            delta_scan_steps: 5,
            vol_model: cs_analytics::InterpolationMode::default(),
            strike_match_mode: cs_domain::StrikeMatchMode::default(),
            max_entry_iv: None,
            wing_width: self.config.wing_width,
            straddle_entry_days: 5,
            straddle_exit_days: 1,
            min_notional: None,
            min_straddle_dte: 7,
            min_entry_price: None,
            max_entry_price: None,
            post_earnings_holding_days: 5,
            hedge_config: cs_domain::HedgeConfig::default(),
            attribution_config: None,
            trading_costs: cs_domain::TradingCostConfig::default(),
            rules: cs_domain::FileRulesConfig::default(),
            return_basis: ReturnBasis::default(),
        };

        // Create repositories
        let earnings_repo = ParquetEarningsRepository::new(data_dir.clone());
        let options_repo = FinqOptionsRepository::new(data_dir.clone());
        let equity_repo = FinqEquityRepository::new(data_dir);

        // Create backtest use case
        let backtest = BacktestUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            rust_config,
        );

        // Run backtest (release GIL during async execution)
        let result = py.allow_threads(|| {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async {
                    backtest.execute(start, end, opt_type, None).await
                })
        });

        let result = result.map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            format!("Backtest failed: {}", e)
        ))?;

        // Convert to Python result
        // Filter to only CalendarSpread results (Python bindings don't support IronButterfly yet)
        Ok(PyBacktestResult {
            results: result.results.into_iter()
                .filter_map(|r| match r {
                    cs_backtest::TradeResult::CalendarSpread(cs) => Some(PyCalendarSpreadResult::from(cs)),
                    cs_backtest::TradeResult::IronButterfly(_) => None,
                })
                .collect(),
            sessions_processed: result.sessions_processed,
            total_entries: result.total_entries,
            total_opportunities: result.total_opportunities,
        })
    }

    fn __repr__(&self) -> String {
        format!("PyBacktestUseCase(config={})", self.config.__repr__())
    }
}
