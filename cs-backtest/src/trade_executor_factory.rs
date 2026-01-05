use std::sync::Arc;
use cs_domain::{
    EquityDataRepository, OptionsDataRepository, TradeFactory,
    HedgeConfig, RollPolicy, RollableTrade,
    CalendarSpread, Straddle, IronButterfly,
};
use cs_analytics::PricingModel;

use crate::execution::{ExecutableTrade, ExecutionConfig};
use crate::trade_executor::TradeExecutor;
use crate::spread_pricer::SpreadPricer;
use crate::straddle_pricer::StraddlePricer;
use crate::iron_butterfly_pricer::IronButterflyPricer;
use crate::timing_strategy::TimingStrategy;

/// Factory for creating type-specific TradeExecutors
///
/// Centralizes dependency management and configuration.
/// Each create_*() method returns a properly configured executor.
pub struct TradeExecutorFactory {
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    trade_factory: Arc<dyn TradeFactory>,
    exec_config: ExecutionConfig,
    pricing_model: PricingModel,
    // Optional configuration
    hedge_config: Option<HedgeConfig>,
    timing_strategy: Option<TimingStrategy>,
    roll_policy: Option<RollPolicy>,
}

impl TradeExecutorFactory {
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        trade_factory: Arc<dyn TradeFactory>,
        exec_config: ExecutionConfig,
    ) -> Self {
        Self {
            options_repo,
            equity_repo,
            trade_factory,
            exec_config,
            pricing_model: PricingModel::default(),
            hedge_config: None,
            timing_strategy: None,
            roll_policy: None,
        }
    }

    pub fn with_pricing_model(mut self, model: PricingModel) -> Self {
        self.pricing_model = model;
        self
    }

    pub fn with_hedging(mut self, config: HedgeConfig, timing: TimingStrategy) -> Self {
        self.hedge_config = Some(config);
        self.timing_strategy = Some(timing);
        self
    }

    pub fn with_roll_policy(mut self, policy: RollPolicy) -> Self {
        self.roll_policy = Some(policy);
        self
    }

    pub fn create_straddle_executor(&self) -> TradeExecutor<Straddle> {
        let pricer = StraddlePricer::new(
            SpreadPricer::new().with_pricing_model(self.pricing_model.clone())
        );
        self.build_executor(pricer)
    }

    pub fn create_calendar_spread_executor(&self) -> TradeExecutor<CalendarSpread> {
        let pricer = SpreadPricer::new().with_pricing_model(self.pricing_model.clone());
        self.build_executor(pricer)
    }

    pub fn create_iron_butterfly_executor(&self) -> TradeExecutor<IronButterfly> {
        let pricer = IronButterflyPricer::new(
            SpreadPricer::new().with_pricing_model(self.pricing_model.clone())
        );
        self.build_executor(pricer)
    }

    fn build_executor<T>(&self, pricer: T::Pricer) -> TradeExecutor<T>
    where
        T: RollableTrade + ExecutableTrade,
    {
        let mut executor = TradeExecutor::<T>::new(
            self.options_repo.clone(),
            self.equity_repo.clone(),
            pricer,
            self.trade_factory.clone(),
            self.exec_config.clone(),
        );

        if let Some(ref policy) = self.roll_policy {
            executor = executor.with_roll_policy(policy.clone());
        }
        if let (Some(ref hc), Some(ref ts)) = (&self.hedge_config, &self.timing_strategy) {
            executor = executor.with_hedging(hc.clone(), ts.clone());
        }

        executor
    }
}
