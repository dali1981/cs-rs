use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use ib_data_collector::database::{ParquetDatabase, DatabaseRepository};
use rust_decimal::Decimal;
use std::path::Path;

use crate::repositories::{EquityDataRepository, RepositoryError};
use crate::value_objects::{EquityBar, SpotPrice};

pub struct IbEquityRepository {
    db: ParquetDatabase,
}

impl IbEquityRepository {
    pub fn new(data_dir: &Path) -> Result<Self, RepositoryError> {
        let db = ParquetDatabase::open(data_dir)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to open IB database at {:?}: {}", data_dir, e)))?;
        Ok(Self { db })
    }
}

#[async_trait]
impl EquityDataRepository for IbEquityRepository {
    async fn get_spot_price(
        &self,
        symbol: &str,
        target_time: DateTime<Utc>,
    ) -> Result<SpotPrice, RepositoryError> {
        let bars = self.get_bars(symbol, target_time.date_naive()).await?;

        let bar = bars.iter()
            .filter(|b| b.timestamp <= target_time)
            .max_by_key(|b| b.timestamp)
            .ok_or_else(|| RepositoryError::NotFound(format!(
                "No equity bar found for {} at or before {}",
                symbol, target_time
            )))?;

        Ok(SpotPrice {
            value: Decimal::try_from(bar.close)
                .map_err(|e| RepositoryError::Parse(format!("Invalid decimal: {}", e)))?,
            timestamp: bar.timestamp,
        })
    }

    async fn get_bars(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<Vec<EquityBar>, RepositoryError> {
        let bars = self.db.read_equity_bars(symbol)
            .map_err(|e| RepositoryError::NotFound(format!("Failed to read equity bars for {}: {}", symbol, e)))?;

        let result: Vec<EquityBar> = bars.into_iter()
            .filter(|b| b.timestamp.date_naive() == date)
            .map(|b| EquityBar {
                close: b.close,
                timestamp: b.timestamp,
            })
            .collect();

        if result.is_empty() {
            return Err(RepositoryError::NotFound(format!(
                "No equity bars found for {} on {}",
                symbol, date
            )));
        }

        Ok(result)
    }
}
