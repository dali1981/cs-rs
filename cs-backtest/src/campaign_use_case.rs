//! Campaign execution use case

use std::sync::Arc;
use tracing::info;

use cs_domain::{
    EarningsRepository, OptionsDataRepository, EquityDataRepository,
    TradingCampaign, RepositoryError,
};

use crate::campaign_config::CampaignConfig;
use crate::session_executor::{SessionExecutor, BatchResult};

pub type CampaignResult<T> = Result<T, CampaignError>;

#[derive(Debug, thiserror::Error)]
pub enum CampaignError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Campaign error: {0}")]
    Other(String),
}

/// Campaign execution use case
///
/// Orchestrates the execution of trading campaigns across multiple symbols
pub struct CampaignUseCase {
    earnings_repo: Arc<dyn EarningsRepository>,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    config: CampaignConfig,
}

impl CampaignUseCase {
    /// Create a new campaign use case
    pub fn new(
        earnings_repo: Box<dyn EarningsRepository>,
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
        config: CampaignConfig,
    ) -> Self {
        Self {
            earnings_repo: Arc::from(earnings_repo),
            options_repo,
            equity_repo,
            config,
        }
    }

    /// Execute the campaign
    ///
    /// Creates campaigns for each symbol, generates sessions, and executes them
    pub async fn execute(&self) -> CampaignResult<BatchResult> {
        info!("Executing campaign for {} symbols", self.config.symbols.len());

        // 1. Load earnings calendar for the date range
        let earnings_calendar = self.load_earnings().await?;

        // 2. Build campaigns (one per symbol)
        let campaigns = self.build_campaigns();

        // 3. Generate all trading sessions
        let mut all_sessions = Vec::new();
        for campaign in &campaigns {
            let sessions = campaign.generate_sessions(&earnings_calendar);
            all_sessions.extend(sessions);
        }

        info!("Generated {} trading sessions", all_sessions.len());

        // 4. Create session executor
        let executor = self.create_executor()?;

        // 5. Execute all sessions
        let result = executor.execute_batch(&all_sessions).await;

        Ok(result)
    }

    /// Load earnings events for the campaign period
    async fn load_earnings(&self) -> CampaignResult<Vec<cs_domain::EarningsEvent>> {
        let events = self.earnings_repo
            .load_earnings(
                self.config.start_date,
                self.config.end_date,
                Some(&self.config.symbols),
            )
            .await?;

        info!("Loaded {} earnings events", events.len());
        Ok(events)
    }

    /// Build trading campaigns from config
    fn build_campaigns(&self) -> Vec<TradingCampaign> {
        self.config.symbols
            .iter()
            .map(|symbol| TradingCampaign {
                symbol: symbol.clone(),
                strategy: self.config.strategy,
                start_date: self.config.start_date,
                end_date: self.config.end_date,
                period_policy: self.config.period_policy.clone(),
                expiration_policy: self.config.expiration_policy.clone(),
                iron_butterfly_config: self.config.iron_butterfly_config.clone(),
                multi_leg_strategy_config: self.config.multi_leg_strategy_config.clone(),
                trade_direction: self.config.trade_direction,
            })
            .collect()
    }

    /// Create session executor with repositories
    fn create_executor(&self) -> CampaignResult<SessionExecutor> {
        use cs_domain::TradeFactory;
        use crate::execution::ExecutionConfig;
        use crate::trade_factory_impl::DefaultTradeFactory;
        use rust_decimal::Decimal;

        // Create trade factory
        let trade_factory: Arc<dyn TradeFactory> = Arc::new(DefaultTradeFactory::new(
            Arc::clone(&self.options_repo),
            Arc::clone(&self.equity_repo),
        ));

        // Create execution config (no IV filtering by default)
        let execution_config = ExecutionConfig {
            max_entry_iv: None,
            min_entry_cost: Decimal::new(50, 2), // $0.50 minimum
            min_credit: None,
            trading_costs: cs_domain::TradingCostConfig::default(),
            hedge_config: None,
            margin_config: cs_domain::MarginConfig::default(),
            attribution_config: None,
        };

        Ok(SessionExecutor::new(
            Arc::clone(&self.options_repo),
            Arc::clone(&self.equity_repo),
            trade_factory,
            execution_config,
        ))
    }
}
