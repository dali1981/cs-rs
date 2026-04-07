//! UseCase factory for creating fully-configured use case instances

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use cs_backtest::{
    BacktestUseCase, BacktestConfig, DataSourceConfig, EarningsSourceConfig,
    CampaignUseCase, CampaignConfig,
    GenerateIvTimeSeriesUseCase,
    MinuteAlignedIvUseCase,

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
            // Infrastructure (not in command)
            data_source,
            data_dir: None,
            earnings_source,

            // Period
            start_date: cmd.period.start_date,
            end_date: cmd.period.end_date,

            // Strategy
            spread: cmd.strategy.spread,
            selection_strategy: cmd.strategy.selection_strategy,
            selection: cmd.strategy.selection,
            timing: cmd.strategy.timing,
            timing_strategy: cmd.strategy.timing_strategy,
            entry_days_before: cmd.strategy.entry_days_before,
            exit_days_before: cmd.strategy.exit_days_before,
            entry_offset: cmd.strategy.entry_offset,
            holding_days: cmd.strategy.holding_days,
            exit_days_after: cmd.strategy.exit_days_after,
            wing_width: cmd.strategy.wing_width,
            straddle_entry_days: cmd.strategy.straddle_entry_days,
            straddle_exit_days: cmd.strategy.straddle_exit_days,
            min_straddle_dte: cmd.strategy.min_straddle_dte,
            post_earnings_holding_days: cmd.strategy.post_earnings_holding_days,

            // Execution
            parallel: cmd.execution.parallel,
            pricing_model: cmd.execution.pricing_model,
            vol_model: cmd.execution.vol_model,
            target_delta: cmd.execution.target_delta,
            delta_range: cmd.execution.delta_range,
            delta_scan_steps: cmd.execution.delta_scan_steps,
            strike_match_mode: cmd.execution.strike_match_mode,

            // Filters
            symbols: cmd.filters.symbols,
            min_market_cap: cmd.filters.min_market_cap,
            max_entry_iv: cmd.filters.max_entry_iv,
            min_notional: cmd.filters.min_notional,
            min_entry_price: cmd.filters.min_entry_price,
            max_entry_price: cmd.filters.max_entry_price,
            rules: cmd.filters.rules,

            // Risk
            return_basis: cmd.risk.return_basis,
            margin: cmd.risk.margin,
            hedge_config: cmd.risk.hedge_config,
            attribution_config: cmd.risk.attribution_config,
            trading_costs: cmd.risk.trading_costs,
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
