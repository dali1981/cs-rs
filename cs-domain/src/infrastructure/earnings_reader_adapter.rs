use async_trait::async_trait;
use chrono::NaiveDate;
use std::path::PathBuf;
use tracing::debug;

use crate::entities::EarningsEvent as DomainEarningsEvent;
use crate::repositories::{EarningsRepository, RepositoryError};
use crate::infrastructure::mappers::IntoNormalized;

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

}

#[async_trait]
impl EarningsRepository for EarningsReaderAdapter {
    async fn load_earnings(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
        symbols: Option<&[String]>,
    ) -> Result<Vec<DomainEarningsEvent>, RepositoryError> {
        debug!("Loading earnings: range={} to {}, symbols={:?}, source={:?}",
            start_date, end_date, symbols, self.source);

        // Build load options
        let mut options = earnings_rs::LoadOptions::new().source(self.source);

        if let Some(syms) = symbols {
            debug!("Filtering earnings for {} symbols: {:?}", syms.len(), syms);
            options = options.symbols(syms.iter().map(|s| s.as_str()));
        }

        // Load events from earnings-rs
        let events = self
            .reader
            .load_range(start_date, end_date, Some(options))
            .map_err(|e| RepositoryError::Parse(format!("earnings-rs error: {}", e)))?;

        debug!("earnings-rs returned {} events", events.len());

        // Translate to domain events via the shared mapper (IntoNormalized)
        let domain_events: Vec<_> = events
            .into_iter()
            .filter_map(|e| e.into_normalized().ok())
            .collect();
        debug!("Converted to {} domain events", domain_events.len());

        Ok(domain_events)
    }
}

// Conversion logic is tested in cs_domain::infrastructure::mappers::earnings
