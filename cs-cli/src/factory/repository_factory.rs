//! Repository factory for creating data access components

#[cfg(all(feature = "full", feature = "demo"))]
compile_error!("Features 'full' and 'demo' are mutually exclusive");

use std::path::PathBuf;

use cs_backtest::EarningsSourceConfig;
use cs_domain::{
    EarningsRepository, OptionsDataRepository, EquityDataRepository,
    infrastructure::ParquetEarningsRepository,
};

// Full mode imports
#[cfg(feature = "full")]
use cs_domain::infrastructure::{
    FinqOptionsRepository, FinqEquityRepository,
    EarningsReaderAdapter,
};

// Demo mode imports
#[cfg(feature = "demo")]
use cs_domain::infrastructure::{
    DemoOptionsRepository, DemoEquityRepository, DemoEarningsRepository,
};

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

// ============================================================================
// Full Mode: Finq-based repositories
// ============================================================================

#[cfg(feature = "full")]
pub struct RepositoryFactory;

#[cfg(feature = "full")]
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

#[cfg(feature = "full")]
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

// ============================================================================
// Demo Mode: Fixture-based repositories
// ============================================================================

#[cfg(feature = "demo")]
pub struct RepositoryFactory;

#[cfg(feature = "demo")]
impl RepositoryFactory {
    /// Create demo options repository (loads from fixtures/)
    pub fn create_options_repo(_data_dir: &PathBuf) -> DemoOptionsRepository {
        DemoOptionsRepository::default()
    }

    /// Create demo equity repository (loads from fixtures/)
    pub fn create_equity_repo(_data_dir: &PathBuf) -> DemoEquityRepository {
        DemoEquityRepository::default()
    }

    /// Create demo earnings repository (loads from fixtures/)
    pub fn create_earnings_repo(
        _earnings_source: &EarningsSourceConfig,
    ) -> Box<dyn EarningsRepository> {
        Box::new(DemoEarningsRepository::default())
    }
}

#[cfg(feature = "demo")]
impl DataRepositoryFactory for RepositoryFactory {
    type OptionsRepo = DemoOptionsRepository;
    type EquityRepo = DemoEquityRepository;

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
