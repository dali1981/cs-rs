use pyo3::prelude::*;
use cs_domain::CalendarSpreadResult;
use finq_core::OptionType;

/// Calendar spread backtest result for a single trade
#[pyclass]
#[derive(Clone)]
pub struct PyCalendarSpreadResult {
    /// Stock symbol
    #[pyo3(get)]
    pub symbol: String,
    /// Earnings date
    #[pyo3(get)]
    pub earnings_date: String,
    /// Earnings time (BMO/AMC/Unknown)
    #[pyo3(get)]
    pub earnings_time: String,
    /// Strike price
    #[pyo3(get)]
    pub strike: f64,
    /// Option type (Call/Put)
    #[pyo3(get)]
    pub option_type: String,
    /// Short leg expiration date
    #[pyo3(get)]
    pub short_expiry: String,
    /// Long leg expiration date
    #[pyo3(get)]
    pub long_expiry: String,
    /// Entry timestamp
    #[pyo3(get)]
    pub entry_time: String,
    /// Short leg entry price
    #[pyo3(get)]
    pub short_entry_price: f64,
    /// Long leg entry price
    #[pyo3(get)]
    pub long_entry_price: f64,
    /// Total entry cost (debit)
    #[pyo3(get)]
    pub entry_cost: f64,
    /// Exit timestamp
    #[pyo3(get)]
    pub exit_time: String,
    /// Short leg exit price
    #[pyo3(get)]
    pub short_exit_price: f64,
    /// Long leg exit price
    #[pyo3(get)]
    pub long_exit_price: f64,
    /// Total exit value
    #[pyo3(get)]
    pub exit_value: f64,
    /// Profit/loss
    #[pyo3(get)]
    pub pnl: f64,
    /// P&L per contract
    #[pyo3(get)]
    pub pnl_per_contract: f64,
    /// P&L percentage
    #[pyo3(get)]
    pub pnl_pct: f64,
    /// Short leg delta at entry
    #[pyo3(get)]
    pub short_delta: Option<f64>,
    /// Short leg gamma at entry
    #[pyo3(get)]
    pub short_gamma: Option<f64>,
    /// Short leg theta at entry
    #[pyo3(get)]
    pub short_theta: Option<f64>,
    /// Short leg vega at entry
    #[pyo3(get)]
    pub short_vega: Option<f64>,
    /// Long leg delta at entry
    #[pyo3(get)]
    pub long_delta: Option<f64>,
    /// Long leg gamma at entry
    #[pyo3(get)]
    pub long_gamma: Option<f64>,
    /// Long leg theta at entry
    #[pyo3(get)]
    pub long_theta: Option<f64>,
    /// Long leg vega at entry
    #[pyo3(get)]
    pub long_vega: Option<f64>,
    /// Short leg IV at entry
    #[pyo3(get)]
    pub iv_short_entry: Option<f64>,
    /// Long leg IV at entry
    #[pyo3(get)]
    pub iv_long_entry: Option<f64>,
    /// Short leg IV at exit
    #[pyo3(get)]
    pub iv_short_exit: Option<f64>,
    /// Long leg IV at exit
    #[pyo3(get)]
    pub iv_long_exit: Option<f64>,
    /// P&L attributed to delta
    #[pyo3(get)]
    pub delta_pnl: Option<f64>,
    /// P&L attributed to gamma
    #[pyo3(get)]
    pub gamma_pnl: Option<f64>,
    /// P&L attributed to theta
    #[pyo3(get)]
    pub theta_pnl: Option<f64>,
    /// P&L attributed to vega
    #[pyo3(get)]
    pub vega_pnl: Option<f64>,
    /// Unexplained P&L
    #[pyo3(get)]
    pub unexplained_pnl: Option<f64>,
    /// Spot price at entry
    #[pyo3(get)]
    pub spot_at_entry: f64,
    /// Spot price at exit
    #[pyo3(get)]
    pub spot_at_exit: f64,
    /// Whether trade was successful
    #[pyo3(get)]
    pub success: bool,
    /// Failure reason if unsuccessful
    #[pyo3(get)]
    pub failure_reason: Option<String>,
}

#[pymethods]
impl PyCalendarSpreadResult {
    /// Check if this trade was a winner (positive P&L)
    pub fn is_winner(&self) -> bool {
        self.pnl > 0.0
    }

    /// Get IV ratio (long IV / short IV) at entry
    fn iv_ratio(&self) -> Option<f64> {
        match (self.iv_long_entry, self.iv_short_entry) {
            (Some(long), Some(short)) if short != 0.0 => Some(long / short),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PyCalendarSpreadResult(symbol='{}', strike={:.2}, pnl={:.2}, pnl_pct={:.2}%)",
            self.symbol, self.strike, self.pnl, self.pnl_pct
        )
    }
}

impl From<CalendarSpreadResult> for PyCalendarSpreadResult {
    fn from(r: CalendarSpreadResult) -> Self {
        Self {
            symbol: r.symbol,
            earnings_date: r.earnings_date.to_string(),
            earnings_time: format!("{:?}", r.earnings_time),
            strike: r.strike.value().to_string().parse().unwrap_or(0.0),
            option_type: if r.option_type == OptionType::Call { "Call".into() } else { "Put".into() },
            short_expiry: r.short_expiry.to_string(),
            long_expiry: r.long_expiry.to_string(),
            entry_time: r.entry_time.to_rfc3339(),
            short_entry_price: r.short_entry_price.to_string().parse().unwrap_or(0.0),
            long_entry_price: r.long_entry_price.to_string().parse().unwrap_or(0.0),
            entry_cost: r.entry_cost.to_string().parse().unwrap_or(0.0),
            exit_time: r.exit_time.to_rfc3339(),
            short_exit_price: r.short_exit_price.to_string().parse().unwrap_or(0.0),
            long_exit_price: r.long_exit_price.to_string().parse().unwrap_or(0.0),
            exit_value: r.exit_value.to_string().parse().unwrap_or(0.0),
            pnl: r.pnl.to_string().parse().unwrap_or(0.0),
            pnl_per_contract: r.pnl_per_contract.to_string().parse().unwrap_or(0.0),
            pnl_pct: r.pnl_pct.to_string().parse().unwrap_or(0.0),
            short_delta: r.short_delta,
            short_gamma: r.short_gamma,
            short_theta: r.short_theta,
            short_vega: r.short_vega,
            long_delta: r.long_delta,
            long_gamma: r.long_gamma,
            long_theta: r.long_theta,
            long_vega: r.long_vega,
            iv_short_entry: r.iv_short_entry,
            iv_long_entry: r.iv_long_entry,
            iv_short_exit: r.iv_short_exit,
            iv_long_exit: r.iv_long_exit,
            delta_pnl: r.delta_pnl.map(|d| d.to_string().parse().unwrap_or(0.0)),
            gamma_pnl: r.gamma_pnl.map(|d| d.to_string().parse().unwrap_or(0.0)),
            theta_pnl: r.theta_pnl.map(|d| d.to_string().parse().unwrap_or(0.0)),
            vega_pnl: r.vega_pnl.map(|d| d.to_string().parse().unwrap_or(0.0)),
            unexplained_pnl: r.unexplained_pnl.map(|d| d.to_string().parse().unwrap_or(0.0)),
            spot_at_entry: r.spot_at_entry,
            spot_at_exit: r.spot_at_exit,
            success: r.success,
            failure_reason: r.failure_reason.map(|f| format!("{:?}", f)),
        }
    }
}
