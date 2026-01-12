//! Repository factory for creating data access components

use std::path::PathBuf;

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
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
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

    /// Create earnings repository based on configuration
    /// Priority: earnings_file > earnings_dir > default location
    pub fn create_earnings_repo(
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
    ) -> Box<dyn EarningsRepository> {
        if let Some(file) = earnings_file {
            // Custom file takes precedence
            Box::new(ParquetEarningsRepository::new(file.clone()))
        } else if let Some(dir) = earnings_dir {
            // Use earnings-rs adapter
            Box::new(EarningsReaderAdapter::new(dir.clone()))
        } else {
            // Default location
            let default_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data");
            Box::new(EarningsReaderAdapter::new(default_dir))
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
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
    ) -> Box<dyn EarningsRepository> {
        RepositoryFactory::create_earnings_repo(earnings_dir, earnings_file)
    }
}
