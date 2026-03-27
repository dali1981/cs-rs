//! Repository factory for creating data access components

use std::path::PathBuf;

use cs_backtest::EarningsSourceConfig;
use cs_domain::{
    EarningsRepository, OptionsDataRepository, EquityDataRepository,
    infrastructure::{
        FinqOptionsRepository, FinqEquityRepository,
        EarningsReaderAdapter, ParquetEarningsRepository,
    },
};

/// Factory for creating repository instances
pub struct RepositoryFactory;

/// Provider trait for building data repositories (pluggable for tests/alt providers)
pub trait DataRepositoryFactory {
    type OptionsRepo: OptionsDataRepository;
    type EquityRepo: EquityDataRepository;

    fn create_options_repo(&self, data_dir: &PathBuf) -> Self::OptionsRepo;
    fn create_equity_repo(&self, data_dir: &PathBuf) -> Self::EquityRepo;
    fn create_earnings_repo(
        &self,
        earnings_source: &EarningsSourceConfig,
    ) -> Box<dyn EarningsRepository>;
}

impl RepositoryFactory {
    /// Create options repository
    pub fn create_options_repo(data_dir: &PathBuf) -> FinqOptionsRepository {
        FinqOptionsRepository::new(data_dir.clone())
    }

    /// Create equity repository
    pub fn create_equity_repo(data_dir: &PathBuf) -> FinqEquityRepository {
        FinqEquityRepository::new(data_dir.clone())
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
                    source.to_earnings_rs()
                ))
            }
        }
    }
}

impl DataRepositoryFactory for RepositoryFactory {
    type OptionsRepo = FinqOptionsRepository;
    type EquityRepo = FinqEquityRepository;

    fn create_options_repo(&self, data_dir: &PathBuf) -> Self::OptionsRepo {
        RepositoryFactory::create_options_repo(data_dir)
    }

    fn create_equity_repo(&self, data_dir: &PathBuf) -> Self::EquityRepo {
        RepositoryFactory::create_equity_repo(data_dir)
    }

    fn create_earnings_repo(
        &self,
        earnings_source: &EarningsSourceConfig,
    ) -> Box<dyn EarningsRepository> {
        RepositoryFactory::create_earnings_repo(earnings_source)
    }
}
