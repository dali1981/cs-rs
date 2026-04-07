//! Test builders for domain entities.
//!
//! Provides fluent builders for constructing domain objects in tests,
//! preventing direct struct initialization and insulating tests from
//! field-name / variant changes. See ADR-0005.

use chrono::NaiveDate;

use crate::entities::EarningsEvent;
use crate::value_objects::EarningsTime;

/// Builder for [`EarningsEvent`] — use in tests instead of direct struct init.
///
/// Default values are intentionally minimal:
/// - `earnings_date`: 2024-01-15
/// - `earnings_time`: `EarningsTime::AfterMarketClose`
/// - All optional fields: `None`
pub struct EarningsEventBuilder {
    symbol: String,
    earnings_date: NaiveDate,
    earnings_time: EarningsTime,
    market_cap: Option<u64>,
    company_name: Option<String>,
}

impl EarningsEventBuilder {
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            earnings_date: NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),
            earnings_time: EarningsTime::AfterMarketClose,
            market_cap: None,
            company_name: None,
        }
    }

    pub fn earnings_date(mut self, date: NaiveDate) -> Self {
        self.earnings_date = date;
        self
    }

    pub fn earnings_time(mut self, time: EarningsTime) -> Self {
        self.earnings_time = time;
        self
    }

    pub fn market_cap(mut self, cap: u64) -> Self {
        self.market_cap = Some(cap);
        self
    }

    pub fn market_cap_opt(mut self, cap: Option<u64>) -> Self {
        self.market_cap = cap;
        self
    }

    pub fn company_name(mut self, name: &str) -> Self {
        self.company_name = Some(name.to_string());
        self
    }

    pub fn build(self) -> EarningsEvent {
        EarningsEvent {
            symbol: self.symbol,
            earnings_date: self.earnings_date,
            earnings_time: self.earnings_time,
            company_name: self.company_name,
            eps_forecast: None,
            market_cap: self.market_cap,
        }
    }
}
