//! UseCase factory for creating fully-configured use case instances

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use cs_backtest::{
    BacktestUseCase, BacktestConfig, DataSourceConfig, EarningsSourceConfig,
    CampaignUseCase, CampaignConfig,
    GenerateIvTimeSeriesUseCase,
    MinuteAlignedIvUseCase,
    EarningsAnalysisUseCase,
    RunBacktestCommand,
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
    /// Create a backtest use case with explicit command and infrastructure config.
    ///
    /// Wires the appropriate repository implementations for the given data source,
    /// then assembles and returns the use case. When `BacktestUseCase` is migrated
    /// to accept `RunBacktestCommand` directly, the internal config reconstruction
    /// can be removed. See ADR-0003.
    #[cfg(feature = "full")]
    pub fn create_backtest(
        command: RunBacktestCommand,
        data_source: DataSourceConfig,
        earnings_source: EarningsSourceConfig,
    ) -> Result<BacktestUseCaseEnum> {
        let config = Self::command_to_config(command, data_source, earnings_source);
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

    /// Create a backtest use case with explicit command and infrastructure config (demo mode)
    #[cfg(feature = "demo")]
    pub fn create_backtest(
        command: RunBacktestCommand,
        data_source: DataSourceConfig,
        earnings_source: EarningsSourceConfig,
    ) -> Result<BacktestUseCase<DemoOptionsRepository, DemoEquityRepository>> {
        let config = Self::command_to_config(command, data_source, earnings_source);
        let factory = RepositoryFactory;
        Self::create_backtest_with_factory(&factory, config)
    }

    /// Reconstruct a `BacktestConfig` from explicit command and infrastructure config.
    ///
    /// Temporary shim until `BacktestUseCase` is migrated to accept
    /// `RunBacktestCommand` directly.
    fn command_to_config(
        cmd: RunBacktestCommand,
        data_source: DataSourceConfig,
        earnings_source: EarningsSourceConfig,
    ) -> BacktestConfig {
        BacktestConfig {
            data_source,
            data_dir: None, // never populated from RunBacktestCommand
            earnings_source,
            start_date: cmd.start_date,
            end_date: cmd.end_date,
            timing: cmd.timing,
            timing_strategy: cmd.timing_strategy,
            entry_days_before: cmd.entry_days_before,
            exit_days_before: cmd.exit_days_before,
            entry_offset: cmd.entry_offset,
            holding_days: cmd.holding_days,
            exit_days_after: cmd.exit_days_after,
            selection: cmd.selection,
            spread: cmd.spread,
            selection_strategy: cmd.selection_strategy,
            symbols: cmd.symbols,
            min_market_cap: cmd.min_market_cap,
            parallel: cmd.parallel,
            pricing_model: cmd.pricing_model,
            vol_model: cmd.vol_model,
            target_delta: cmd.target_delta,
            delta_range: cmd.delta_range,
            delta_scan_steps: cmd.delta_scan_steps,
            strike_match_mode: cmd.strike_match_mode,
            max_entry_iv: cmd.max_entry_iv,
            wing_width: cmd.wing_width,
            straddle_entry_days: cmd.straddle_entry_days,
            straddle_exit_days: cmd.straddle_exit_days,
            min_notional: cmd.min_notional,
            min_straddle_dte: cmd.min_straddle_dte,
            min_entry_price: cmd.min_entry_price,
            max_entry_price: cmd.max_entry_price,
            post_earnings_holding_days: cmd.post_earnings_holding_days,
            hedge_config: cmd.hedge_config,
            attribution_config: cmd.attribution_config,
            trading_costs: cmd.trading_costs,
            rules: cmd.rules,
            return_basis: cmd.return_basis,
            margin: cmd.margin,
        }
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
