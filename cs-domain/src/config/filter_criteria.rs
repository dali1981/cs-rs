/// Criteria for filtering tradable events
///
/// These filters determine which events/trades to include in backtests or campaigns.
/// They capture business rules about trade quality and liquidity requirements.
#[derive(Debug, Clone, Default)]
pub struct FilterCriteria {
    /// Only trade specific symbols (None = all symbols)
    pub symbols: Option<Vec<String>>,

    /// Minimum market capitalization (filters small-cap stocks)
    pub min_market_cap: Option<u64>,

    /// Maximum implied volatility at entry (filters unreliable pricing)
    /// Common values: 1.5 (150%), 2.0 (200%)
    pub max_entry_iv: Option<f64>,

    /// Minimum daily option notional: sum(volumes) × 100 × stock_price
    /// Measures total dollar liquidity in options traded that day
    /// Example: Some(100_000.0) = require $100k minimum daily option activity
    pub min_notional: Option<f64>,

    /// Minimum entry price (total debit/credit for the position)
    pub min_entry_price: Option<f64>,

    /// Maximum entry price (caps maximum loss exposure)
    pub max_entry_price: Option<f64>,

    /// Minimum IV ratio (short/long or near/far)
    /// Used for calendar spreads to ensure IV curve steepness
    pub min_iv_ratio: Option<f64>,
}

impl FilterCriteria {
    /// Create empty filter (no filtering)
    pub fn none() -> Self {
        Self::default()
    }

    /// Create filter for specific symbols only
    pub fn symbols(symbols: Vec<String>) -> Self {
        Self {
            symbols: Some(symbols),
            ..Default::default()
        }
    }

    /// Check if symbol passes symbol filter
    pub fn symbol_matches(&self, symbol: &str) -> bool {
        match &self.symbols {
            None => true, // No filter = accept all
            Some(allowed) => allowed.iter().any(|s| s.eq_ignore_ascii_case(symbol)),
        }
    }

    /// Check if market cap passes filter
    pub fn market_cap_matches(&self, market_cap: Option<u64>) -> bool {
        match (self.min_market_cap, market_cap) {
            (Some(min), Some(cap)) => cap >= min,
            (Some(_), None) => false, // Required but missing = reject
            (None, _) => true,        // No filter = accept all
        }
    }

    /// Check if IV passes filter
    pub fn iv_matches(&self, iv: f64) -> bool {
        match self.max_entry_iv {
            Some(max) => iv <= max,
            None => true,
        }
    }

    /// Check if notional passes filter
    pub fn notional_matches(&self, notional: f64) -> bool {
        match self.min_notional {
            Some(min) => notional >= min,
            None => true,
        }
    }

    /// Check if entry price passes filter
    pub fn entry_price_matches(&self, price: f64) -> bool {
        let min_ok = self.min_entry_price.map_or(true, |min| price >= min);
        let max_ok = self.max_entry_price.map_or(true, |max| price <= max);
        min_ok && max_ok
    }

    /// Check if IV ratio passes filter
    pub fn iv_ratio_matches(&self, ratio: f64) -> bool {
        match self.min_iv_ratio {
            Some(min) => ratio >= min,
            None => true,
        }
    }
}
