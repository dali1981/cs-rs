use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    SpotPrice, Straddle, TradeFactory, TradeFactoryError,
};
use cs_domain::strike_selection::{ATMStrategy, StrikeSelector};

/// Default implementation of TradeFactory using ATM selection strategy
///
/// This factory queries option chains and equity data to construct valid
/// trades using real market data. It uses the ATM selection strategy to
/// find the strike closest to spot price and selects the nearest available
/// expiration that meets minimum DTE requirements.
pub struct DefaultTradeFactory<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    options_repo: Arc<O>,
    equity_repo: Arc<E>,
    selector: ATMStrategy,
}

impl<O, E> DefaultTradeFactory<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    /// Create a new trade factory with default ATM selection strategy
    pub fn new(options_repo: Arc<O>, equity_repo: Arc<E>) -> Self {
        Self {
            options_repo,
            equity_repo,
            selector: ATMStrategy::default(),
        }
    }

    /// Create a trade factory with custom ATM strategy configuration
    pub fn with_selector(mut self, selector: ATMStrategy) -> Self {
        self.selector = selector;
        self
    }
}

#[async_trait]
impl<O, E> TradeFactory for DefaultTradeFactory<O, E>
where
    O: OptionsDataRepository,
    E: EquityDataRepository,
{
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Straddle, TradeFactoryError> {
        // 1. Query option chain from repository
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface from option chain data
        // This extracts available strikes and expirations from real market data
        // The build function will query the equity repo internally to get spot price
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Use strike selector to find ATM straddle with real expiration
        // The selector will:
        // - Find the strike closest to spot price
        // - Filter expirations to those AFTER min_expiration
        // - Select the soonest valid expiration
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        self.selector
            .select_straddle(&spot_price, &surface, min_expiration)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Strike selection failed: {}", e)))
    }

    async fn available_expirations(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<NaiveDate>, TradeFactoryError> {
        // Query option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // Build IV surface to extract available expirations
        // The build function queries equity repo internally for spot price
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // Return sorted list of expirations from the IV surface
        Ok(surface.expirations())
    }
}
