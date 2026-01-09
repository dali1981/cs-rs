//! Types for generic trade execution

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;
use cs_domain::{OptionStrategy, RepositoryError, TradingCostConfig, TradingCostCalculator, HedgeConfig};
use crate::spread_pricer::PricingError;

/// Errors that can occur during trade execution
#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("Repository error: {0}")]
    Repository(#[from] RepositoryError),
    #[error("Pricing error: {0}")]
    Pricing(#[from] PricingError),
    #[error("No spot price available")]
    NoSpotPrice,
    #[error("Invalid spread: {0}")]
    InvalidSpread(String),
}

/// Configuration for trade validation
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Maximum allowed IV at entry (filters unreliable pricing)
    pub max_entry_iv: Option<f64>,

    /// Minimum entry cost to avoid near-zero pricing
    pub min_entry_cost: Decimal,

    /// Minimum credit for credit spreads (optional)
    pub min_credit: Option<Decimal>,

    /// Trading costs configuration (slippage + commission)
    pub trading_costs: TradingCostConfig,

    /// Delta hedging configuration (None = hedging disabled)
    pub hedge_config: Option<HedgeConfig>,
}

impl ExecutionConfig {
    /// Create config for straddle execution
    pub fn for_straddle(max_entry_iv: Option<f64>) -> Self {
        Self {
            max_entry_iv,
            min_entry_cost: Decimal::new(50, 2), // $0.50 minimum for straddles
            min_credit: None,
            trading_costs: TradingCostConfig::default(),
            hedge_config: None,
        }
    }

    /// Create config for calendar spread execution
    pub fn for_calendar_spread(max_entry_iv: Option<f64>) -> Self {
        Self {
            max_entry_iv,
            min_entry_cost: Decimal::new(5, 2), // $0.05 minimum for calendar spreads
            min_credit: None,
            trading_costs: TradingCostConfig::default(),
            hedge_config: None,
        }
    }

    /// Create config for iron butterfly execution (credit spread)
    pub fn for_iron_butterfly(max_entry_iv: Option<f64>) -> Self {
        Self {
            max_entry_iv,
            min_entry_cost: Decimal::new(10, 2), // $0.10 minimum credit for iron butterflies
            min_credit: Some(Decimal::new(10, 2)),
            trading_costs: TradingCostConfig::default(),
            hedge_config: None,
        }
    }

    /// Create config for calendar straddle execution (debit spread)
    pub fn for_calendar_straddle(max_entry_iv: Option<f64>) -> Self {
        Self {
            max_entry_iv,
            min_entry_cost: Decimal::new(50, 2), // $0.50 minimum for calendar straddles (like straddles)
            min_credit: None,
            trading_costs: TradingCostConfig::default(),
            hedge_config: None,
        }
    }

    /// Enable hedging on this config (builder pattern)
    pub fn with_hedging(mut self, hedge_config: HedgeConfig) -> Self {
        self.hedge_config = Some(hedge_config);
        self
    }

    /// Check if hedging is enabled
    pub fn has_hedging(&self) -> bool {
        self.hedge_config.as_ref().map_or(false, |h| !matches!(h.strategy, cs_domain::HedgeStrategy::None))
    }

    /// Set custom trading costs configuration
    pub fn with_trading_costs(mut self, trading_costs: TradingCostConfig) -> Self {
        self.trading_costs = trading_costs;
        self
    }

    /// Build the trading cost calculator from config
    pub fn cost_calculator(&self) -> Box<dyn TradingCostCalculator> {
        self.trading_costs.build()
    }

    /// Check if trading costs are configured (non-zero)
    pub fn has_trading_costs(&self) -> bool {
        self.trading_costs.has_costs()
    }

    /// Create strategy-specific config based on option strategy type
    ///
    /// This factory method maps each OptionStrategy to its correct ExecutionConfig
    /// with appropriate validation thresholds.
    pub fn for_strategy(strategy: OptionStrategy, max_entry_iv: Option<f64>) -> Self {
        match strategy {
            OptionStrategy::CalendarSpread => Self::for_calendar_spread(max_entry_iv),
            OptionStrategy::IronButterfly => Self::for_iron_butterfly(max_entry_iv),
            OptionStrategy::Straddle => Self::for_straddle(max_entry_iv),
            OptionStrategy::CalendarStraddle => Self::for_calendar_straddle(max_entry_iv),
            // Multi-leg strategies: use iron butterfly config (credit spreads with wings)
            OptionStrategy::Strangle => Self::for_iron_butterfly(max_entry_iv),
            OptionStrategy::Butterfly => Self::for_iron_butterfly(max_entry_iv),
            OptionStrategy::Condor => Self::for_iron_butterfly(max_entry_iv),
            OptionStrategy::IronCondor => Self::for_iron_butterfly(max_entry_iv),
        }
    }
}

/// Output from trade simulation
///
/// Contains pure simulation data: spots, times, surfaces.
/// Does NOT contain business context (earnings events) - that's the caller's responsibility.
#[derive(Debug)]
pub struct SimulationOutput {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_spot: f64,
    pub exit_spot: f64,
    pub entry_surface_time: Option<DateTime<Utc>>,
    pub exit_surface_time: DateTime<Utc>,
}

impl SimulationOutput {
    /// Create a new simulation output
    pub fn new(
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        entry_spot: f64,
        exit_spot: f64,
        entry_surface_time: Option<DateTime<Utc>>,
        exit_surface_time: DateTime<Utc>,
    ) -> Self {
        Self {
            entry_time,
            exit_time,
            entry_spot,
            exit_spot,
            entry_surface_time,
            exit_surface_time,
        }
    }
}
