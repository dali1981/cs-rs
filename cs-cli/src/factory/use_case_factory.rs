//! UseCase factory for creating fully-configured use case instances

use anyhow::Result;
use std::path::PathBuf;

use cs_backtest::{
    BacktestUseCase, BacktestConfig,
    GenerateIvTimeSeriesUseCase,
    EarningsAnalysisUseCase,
};
use cs_domain::infrastructure::{FinqOptionsRepository, FinqEquityRepository};

use super::RepositoryFactory;

/// Factory for creating use case instances with all dependencies wired up
pub struct UseCaseFactory;

impl UseCaseFactory {
    /// Create a backtest use case with all dependencies
    /// Earnings repos are constructed from config.earnings_file and config.earnings_dir
    pub fn create_backtest(
        config: BacktestConfig,
    ) -> Result<BacktestUseCase<FinqOptionsRepository, FinqEquityRepository>> {
        let options_repo = RepositoryFactory::create_options_repo(&config.data_dir);
        let equity_repo = RepositoryFactory::create_equity_repo(&config.data_dir);

        // Get earnings file/dir from config
        let earnings_repo = RepositoryFactory::create_earnings_repo(
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

    /// Create ATM IV generation use case
    pub fn create_atm_iv(
        data_dir: &PathBuf,
    ) -> Result<GenerateIvTimeSeriesUseCase<FinqEquityRepository, FinqOptionsRepository>> {
        let equity_repo = RepositoryFactory::create_equity_repo(data_dir);
        let options_repo = RepositoryFactory::create_options_repo(data_dir);

        Ok(GenerateIvTimeSeriesUseCase::new(equity_repo, options_repo))
    }
}
