// ! IB repository factory for creating IB data access components

use std::path::PathBuf;

use cs_backtest::EarningsSourceConfig;
use cs_domain::{
    infrastructure::{
        EarningsReaderAdapter, IbEquityRepository, IbOptionsRepository, ParquetEarningsRepository,
    },
    EarningsRepository,
};

use super::repository_factory::DataRepositoryFactory;

/// Factory for creating IB repository instances
pub struct IbRepositoryFactory;

impl IbRepositoryFactory {
    /// Create IB options repository
    pub fn create_options_repo(data_dir: &PathBuf) -> Result<IbOptionsRepository, String> {
        IbOptionsRepository::new(data_dir)
            .map_err(|e| format!("Failed to create IB options repository: {}", e))
    }

    /// Create IB equity repository
    pub fn create_equity_repo(data_dir: &PathBuf) -> Result<IbEquityRepository, String> {
        IbEquityRepository::new(data_dir)
            .map_err(|e| format!("Failed to create IB equity repository: {}", e))
    }

    /// Create earnings repository based on unified configuration
    pub fn create_earnings_repo(
        earnings_source: &EarningsSourceConfig,
    ) -> Box<dyn EarningsRepository> {
        match earnings_source {
            EarningsSourceConfig::File { path } => {
                // Custom file (parquet)
                Box::new(ParquetEarningsRepository::new(path.clone()))
            }
            EarningsSourceConfig::Provider { dir, source } => {
                // Use earnings-rs adapter with configured source
                Box::new(EarningsReaderAdapter::with_source(
                    dir.clone(),
                    source.to_earnings_rs(),
                ))
            }
        }
    }
}

impl DataRepositoryFactory for IbRepositoryFactory {
    type OptionsRepo = IbOptionsRepository;
    type EquityRepo = IbEquityRepository;

    fn create_options_repo(&self, data_dir: &PathBuf) -> Self::OptionsRepo {
        IbRepositoryFactory::create_options_repo(data_dir)
            .expect("Failed to create IB options repository")
    }

    fn create_equity_repo(&self, data_dir: &PathBuf) -> Self::EquityRepo {
        IbRepositoryFactory::create_equity_repo(data_dir)
            .expect("Failed to create IB equity repository")
    }

    fn create_earnings_repo(
        &self,
        earnings_source: &EarningsSourceConfig,
    ) -> Box<dyn EarningsRepository> {
        IbRepositoryFactory::create_earnings_repo(earnings_source)
    }
}
