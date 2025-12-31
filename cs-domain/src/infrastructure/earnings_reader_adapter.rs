use async_trait::async_trait;
use chrono::NaiveDate;
use std::path::PathBuf;

use crate::entities::EarningsEvent as DomainEarningsEvent;
use crate::repositories::{EarningsRepository, RepositoryError};
use crate::value_objects::EarningsTime;

/// Adapter that wraps earnings-rs EarningsReader to implement EarningsRepository
pub struct EarningsReaderAdapter {
    reader: earnings_rs::EarningsReader,
    source: earnings_rs::DataSource,
}

impl EarningsReaderAdapter {
    /// Create a new adapter with the given data directory
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            reader: earnings_rs::EarningsReader::new(data_dir),
            source: earnings_rs::DataSource::TradingView,
        }
    }

    /// Create with custom data source
    pub fn with_source(data_dir: PathBuf, source: earnings_rs::DataSource) -> Self {
        Self {
            reader: earnings_rs::EarningsReader::new(data_dir),
            source,
        }
    }

    /// Convert earnings-rs ReportTime to domain EarningsTime
    fn convert_report_time(report_time: earnings_rs::ReportTime) -> EarningsTime {
        match report_time {
            earnings_rs::ReportTime::BeforeMarket => EarningsTime::BeforeMarketOpen,
            earnings_rs::ReportTime::AfterMarket => EarningsTime::AfterMarketClose,
            earnings_rs::ReportTime::DuringMarket | earnings_rs::ReportTime::NotSupplied => {
                EarningsTime::Unknown
            }
        }
    }

    /// Convert earnings-rs EarningsEvent to domain EarningsEvent
    fn convert_event(event: earnings_rs::EarningsEvent) -> DomainEarningsEvent {
        let earnings_time = Self::convert_report_time(event.report_time);

        // Convert market cap from millions (Decimal) to whole number (u64)
        let market_cap = event.market_cap_millions.and_then(|cap_millions| {
            let cap_millions_f64: f64 = cap_millions.try_into().ok()?;
            let cap_whole = (cap_millions_f64 * 1_000_000.0) as u64;
            Some(cap_whole)
        });

        let mut domain_event = DomainEarningsEvent::new(
            event.symbol,
            event.report_date,
            earnings_time,
        );

        if !event.company_name.is_empty() {
            domain_event = domain_event.with_company_name(event.company_name);
        }

        if let Some(cap) = market_cap {
            domain_event = domain_event.with_market_cap(cap);
        }

        domain_event
    }
}

#[async_trait]
impl EarningsRepository for EarningsReaderAdapter {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<DomainEarningsEvent>, RepositoryError> {
        // Build load options
        let mut options = earnings_rs::LoadOptions::new().source(self.source);

        if let Some(syms) = symbols {
            options = options.symbols(syms.iter().map(|s| s.as_str()));
        }

        // Load events from earnings-rs
        let events = self
            .reader
            .load_range(start_date, end_date, Some(options))
            .map_err(|e| RepositoryError::Parse(format!("earnings-rs error: {}", e)))?;

        // Convert to domain events
        Ok(events.into_iter().map(Self::convert_event).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_report_time() {
        assert_eq!(
            EarningsReaderAdapter::convert_report_time(earnings_rs::ReportTime::BeforeMarket),
            EarningsTime::BeforeMarketOpen
        );
        assert_eq!(
            EarningsReaderAdapter::convert_report_time(earnings_rs::ReportTime::AfterMarket),
            EarningsTime::AfterMarketClose
        );
        assert_eq!(
            EarningsReaderAdapter::convert_report_time(earnings_rs::ReportTime::NotSupplied),
            EarningsTime::Unknown
        );
    }

    #[test]
    fn test_convert_event() {
        use rust_decimal::Decimal;

        let earnings_event = earnings_rs::EarningsEvent {
            symbol: "AAPL".into(),
            company_name: "Apple Inc.".into(),
            report_date: NaiveDate::from_ymd_opt(2025, 11, 4).unwrap(),
            report_time: earnings_rs::ReportTime::AfterMarket,
            fiscal_quarter_ending: None,
            eps_forecast: None,
            eps_actual: None,
            last_year_eps: None,
            surprise_pct: None,
            market_cap_millions: Some(Decimal::from(3000000)), // $3T
            num_of_estimates: None,
        };

        let domain_event = EarningsReaderAdapter::convert_event(earnings_event);

        assert_eq!(domain_event.symbol, "AAPL");
        assert_eq!(domain_event.company_name, Some("Apple Inc.".into()));
        assert_eq!(domain_event.earnings_time, EarningsTime::AfterMarketClose);
        assert_eq!(domain_event.market_cap, Some(3_000_000_000_000));
    }
}
