//! UseCase factory for creating fully-configured use case instances

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use cs_backtest::{
    BacktestUseCase, BacktestConfig, DataSourceConfig,
    CampaignUseCase, CampaignConfig,
    GenerateIvTimeSeriesUseCase,
    MinuteAlignedIvUseCase,
    EarningsAnalysisUseCase,
};
use cs_domain::{OptionsDataRepository, EquityDataRepository};

// Full mode types
#[cfg(feature = "full")]
use cs_domain::infrastructure::{
    FinqOptionsRepository, FinqEquityRepository,
    IbOptionsRepository, IbEquityRepository,
};

// Demo mode types
#[cfg(feature = "demo")]
use cs_domain::infrastructure::{DemoOptionsRepository, DemoEquityRepository};

#[cfg(feature = "full")]
use super::{RepositoryFactory, IbRepositoryFactory, DataRepositoryFactory};
#[cfg(feature = "demo")]
use super::{RepositoryFactory, DataRepositoryFactory};

/// Enum wrapper for backtest use cases with different repository types
#[cfg(feature = "full")]
pub enum BacktestUseCaseEnum {
    Finq(BacktestUseCase<FinqOptionsRepository, FinqEquityRepository>),
    Ib(BacktestUseCase<IbOptionsRepository, IbEquityRepository>),
}

#[cfg(feature = "full")]
impl BacktestUseCaseEnum {
    pub async fn execute(&self) -> Result<cs_backtest::UnifiedBacktestResult, cs_backtest::BacktestError> {
        match self {
            Self::Finq(uc) => uc.execute().await,
            Self::Ib(uc) => uc.execute().await,
        }
    }
}

/// Factory for creating use case instances with all dependencies wired up
pub struct UseCaseFactory;

impl UseCaseFactory {
    /// Create a backtest use case with all dependencies
    /// Dispatches to appropriate repository factory based on config.data_source
    /// Earnings repos are constructed from config.earnings_file and config.earnings_dir
    #[cfg(feature = "full")]
    pub fn create_backtest(
        config: BacktestConfig,
    ) -> Result<BacktestUseCaseEnum> {
        match &config.data_source {
            DataSourceConfig::Finq { data_dir: _ } => {
                let factory = RepositoryFactory;
                let use_case = Self::create_backtest_with_factory(&factory, config)?;
                Ok(BacktestUseCaseEnum::Finq(use_case))
            }
            DataSourceConfig::Ib { data_dir: _ } => {
                let factory = IbRepositoryFactory;
                let use_case = Self::create_backtest_with_factory(&factory, config)?;
                Ok(BacktestUseCaseEnum::Ib(use_case))
            }
        }
    }

    /// Create a backtest use case with all dependencies (demo mode)
    #[cfg(feature = "demo")]
    pub fn create_backtest(
        config: BacktestConfig,
    ) -> Result<BacktestUseCase<DemoOptionsRepository, DemoEquityRepository>> {
        let factory = RepositoryFactory;
        Self::create_backtest_with_factory(&factory, config)
    }

    /// Create a backtest use case with a custom repository factory
    pub fn create_backtest_with_factory<F>(
        factory: &F,
        config: BacktestConfig,
    ) -> Result<BacktestUseCase<F::OptionsRepo, F::EquityRepo>>
    where
        F: DataRepositoryFactory,
        F::OptionsRepo: OptionsDataRepository + 'static,
        F::EquityRepo: EquityDataRepository + 'static,
    {
        let data_dir = config.data_source.data_dir();
        let options_repo = factory.create_options_repo(data_dir);
        let equity_repo = factory.create_equity_repo(data_dir);

        // Create earnings repository from unified config
        let earnings_repo = factory.create_earnings_repo(&config.earnings_source);

        Ok(BacktestUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            config,
        ))
    }

    /// Create a campaign use case with all dependencies
    #[cfg(any(feature = "full", feature = "demo"))]
    pub fn create_campaign(
        config: CampaignConfig,
    ) -> Result<CampaignUseCase> {
        let options_repo = Arc::new(RepositoryFactory::create_options_repo(&config.data_dir));
        let equity_repo = Arc::new(RepositoryFactory::create_equity_repo(&config.data_dir));

        // Create earnings repository from unified config
        let earnings_repo = RepositoryFactory::create_earnings_repo(&config.earnings_source);

        Ok(CampaignUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            config,
        ))
    }

    /// Create ATM IV generation use case
    #[cfg(feature = "full")]
    pub fn create_atm_iv(
        data_dir: &PathBuf,
    ) -> Result<GenerateIvTimeSeriesUseCase<FinqEquityRepository, FinqOptionsRepository>> {
        let factory = RepositoryFactory;
        Self::create_atm_iv_with_factory(&factory, data_dir)
    }

    /// Create ATM IV generation use case (demo mode)
    #[cfg(feature = "demo")]
    pub fn create_atm_iv(
        data_dir: &PathBuf,
    ) -> Result<GenerateIvTimeSeriesUseCase<DemoEquityRepository, DemoOptionsRepository>> {
        let factory = RepositoryFactory;
        Self::create_atm_iv_with_factory(&factory, data_dir)
    }

    /// Create ATM IV generation use case with a custom repository factory
    pub fn create_atm_iv_with_factory<F>(
        factory: &F,
        data_dir: &PathBuf,
    ) -> Result<GenerateIvTimeSeriesUseCase<F::EquityRepo, F::OptionsRepo>>
    where
        F: DataRepositoryFactory,
        F::OptionsRepo: OptionsDataRepository,
        F::EquityRepo: EquityDataRepository,
    {
        let equity_repo = factory.create_equity_repo(data_dir);
        let options_repo = factory.create_options_repo(data_dir);

        Ok(GenerateIvTimeSeriesUseCase::new(equity_repo, options_repo))
    }

    /// Create minute-aligned IV generation use case (default mode)
    #[cfg(feature = "full")]
    pub fn create_minute_aligned_iv(
        data_dir: &PathBuf,
    ) -> Result<MinuteAlignedIvUseCase<FinqEquityRepository, FinqOptionsRepository>> {
        let factory = RepositoryFactory;
        Self::create_minute_aligned_iv_with_factory(&factory, data_dir)
    }

    /// Create minute-aligned IV generation use case (demo mode)
    #[cfg(feature = "demo")]
    pub fn create_minute_aligned_iv(
        data_dir: &PathBuf,
    ) -> Result<MinuteAlignedIvUseCase<DemoEquityRepository, DemoOptionsRepository>> {
        let factory = RepositoryFactory;
        Self::create_minute_aligned_iv_with_factory(&factory, data_dir)
    }

    /// Create minute-aligned IV generation use case with a custom repository factory
    pub fn create_minute_aligned_iv_with_factory<F>(
        factory: &F,
        data_dir: &PathBuf,
    ) -> Result<MinuteAlignedIvUseCase<F::EquityRepo, F::OptionsRepo>>
    where
        F: DataRepositoryFactory,
        F::OptionsRepo: OptionsDataRepository,
        F::EquityRepo: EquityDataRepository,
    {
        let equity_repo = factory.create_equity_repo(data_dir);
        let options_repo = factory.create_options_repo(data_dir);

        Ok(MinuteAlignedIvUseCase::new(equity_repo, options_repo))
    }
}
