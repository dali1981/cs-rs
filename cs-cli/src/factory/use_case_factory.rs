//! UseCase factory for creating fully-configured use case instances

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use cs_backtest::{
    BacktestUseCase, BacktestConfig,
    CampaignUseCase, CampaignConfig,
    GenerateIvTimeSeriesUseCase,
    EarningsAnalysisUseCase,
};
use cs_domain::{OptionsDataRepository, EquityDataRepository};
use cs_domain::infrastructure::{FinqOptionsRepository, FinqEquityRepository};

use super::{RepositoryFactory, DataRepositoryFactory};

/// Factory for creating use case instances with all dependencies wired up
pub struct UseCaseFactory;

impl UseCaseFactory {
    /// Create a backtest use case with all dependencies
    /// Earnings repos are constructed from config.earnings_file and config.earnings_dir
    pub fn create_backtest(
        config: BacktestConfig,
    ) -> Result<BacktestUseCase<FinqOptionsRepository, FinqEquityRepository>> {
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
        let options_repo = factory.create_options_repo(&config.data_dir);
        let equity_repo = factory.create_equity_repo(&config.data_dir);

        // Get earnings file/dir from config
        let earnings_repo = factory.create_earnings_repo(
            Some(&config.earnings_dir),
            config.earnings_file.as_ref(),
        );

        Ok(BacktestUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            config,
        ))
    }

    /// Create a campaign use case with all dependencies
    /// Earnings repos are constructed from config.earnings_file and config.earnings_dir
    pub fn create_campaign(
        config: CampaignConfig,
    ) -> Result<CampaignUseCase> {
        let options_repo = Arc::new(RepositoryFactory::create_options_repo(&config.data_dir));
        let equity_repo = Arc::new(RepositoryFactory::create_equity_repo(&config.data_dir));

        // Get earnings file/dir from config
        let earnings_repo = RepositoryFactory::create_earnings_repo(
            Some(&config.earnings_dir),
            config.earnings_file.as_ref(),
        );

        Ok(CampaignUseCase::new(
            earnings_repo,
            options_repo,
            equity_repo,
            config,
        ))
    }

    /// Create ATM IV generation use case
    pub fn create_atm_iv(
        data_dir: &PathBuf,
    ) -> Result<GenerateIvTimeSeriesUseCase<FinqEquityRepository, FinqOptionsRepository>> {
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
}
