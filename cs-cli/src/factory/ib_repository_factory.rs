// ! IB repository factory for creating IB data access components

use std::path::PathBuf;

use cs_domain::{
    EarningsRepository, OptionsDataRepository, EquityDataRepository,
    infrastructure::{
        IbOptionsRepository, IbEquityRepository,
        EarningsReaderAdapter, ParquetEarningsRepository,
    },
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

    /// Create earnings repository (same as Finq - earnings data is provider-agnostic)
    pub fn create_earnings_repo(
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
    ) -> Box<dyn EarningsRepository> {
        if let Some(file) = earnings_file {
            Box::new(ParquetEarningsRepository::new(file.clone()))
        } else if let Some(dir) = earnings_dir {
            Box::new(EarningsReaderAdapter::new(dir.clone()))
        } else {
            let default_dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("trading_project/nasdaq_earnings/data");
            Box::new(EarningsReaderAdapter::new(default_dir))
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
        earnings_dir: Option<&PathBuf>,
        earnings_file: Option<&PathBuf>,
    ) -> Box<dyn EarningsRepository> {
        IbRepositoryFactory::create_earnings_repo(earnings_dir, earnings_file)
    }
}
