use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;

use cs_domain::{
    EquityDataRepository, OptionsDataRepository,
    SpotPrice, LongStraddle, CalendarSpread, IronButterfly, TradeFactory, TradeFactoryError,
};
use cs_domain::strike_selection::ExpirationCriteria;
use crate::strike_selection::{ATMStrategy, StrikeSelector};
use finq_core::OptionType;

/// Default implementation of TradeFactory using ATM selection strategy
///
/// This factory queries option chains and equity data to construct valid
/// trades using real market data. It uses the ATM selection strategy to
/// find the strike closest to spot price and selects the nearest available
/// expiration that meets minimum DTE requirements.
pub struct DefaultTradeFactory {
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    selector: ATMStrategy,
}

impl DefaultTradeFactory {
    /// Create a new trade factory with default ATM selection strategy
    pub fn new(
        options_repo: Arc<dyn OptionsDataRepository>,
        equity_repo: Arc<dyn EquityDataRepository>,
    ) -> Self {
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
impl TradeFactory for DefaultTradeFactory {
    async fn create_atm_straddle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<LongStraddle, TradeFactoryError> {
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
            .select_long_straddle(&spot_price, &surface, min_expiration)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Strike selection failed: {}", e)))
    }

    async fn create_calendar_spread(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_short_dte: u32,
        max_short_dte: u32,
        min_long_dte: u32,
        option_type: OptionType,
    ) -> Result<CalendarSpread, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Create expiration criteria from DTE parameters
        let criteria = ExpirationCriteria::new(
            min_short_dte as i32,
            max_short_dte as i32,
            min_long_dte as i32,
            120, // max_long_dte: allow up to 120 DTE for long leg
        );

        // 5. Use selector to build calendar spread
        self.selector
            .select_calendar_spread(
                &spot_price,
                &surface,
                option_type,
                &criteria,
            )
            .map_err(|e| TradeFactoryError::SelectionError(format!("Calendar spread selection failed: {}", e)))
    }

    async fn create_iron_butterfly(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        wing_width: Decimal,
    ) -> Result<IronButterfly, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Compute DTE range from min_expiration
        // min_expiration is the earliest acceptable expiration, compute DTE from as_of
        let as_of_date = as_of.naive_utc().date();
        let min_dte = (min_expiration - as_of_date).num_days() as i32;
        // Use a typical DTE range for iron butterflies: min_dte to min_dte + 15 days
        // This ensures we stay near the minimum required expiration
        let max_dte = min_dte + 15;

        // 5. Use selector to build iron butterfly
        self.selector
            .select_iron_butterfly(&spot_price, &surface, wing_width, min_dte, max_dte)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Iron butterfly selection failed: {}", e)))
    }

    async fn create_iron_butterfly_advanced(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &cs_domain::value_objects::IronButterflyConfig,
        direction: cs_domain::value_objects::TradeDirection,
    ) -> Result<IronButterfly, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Compute DTE range from min_expiration
        let as_of_date = as_of.naive_utc().date();
        let min_dte = (min_expiration - as_of_date).num_days() as i32;
        let max_dte = min_dte + 15;

        // 5. Use selector to build advanced iron butterfly
        self.selector
            .select_iron_butterfly_with_config(&spot_price, &surface, config, direction, min_dte, max_dte)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Advanced iron butterfly selection failed: {}", e)))
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

    async fn create_strangle(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &cs_domain::value_objects::MultiLegStrategyConfig,
    ) -> Result<cs_domain::entities::Strangle, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Compute DTE range from min_expiration
        let as_of_date = as_of.naive_utc().date();
        let min_dte = (min_expiration - as_of_date).num_days() as i32;
        let max_dte = min_dte + 15;

        // 5. Use selector to build strangle
        let selection = self.selector.select_multi_leg(&spot_price, &surface, config, min_dte, max_dte)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Strangle selection failed: {}", e)))?;

        // 6. Extract strikes
        let far_strikes = selection.far_strikes.ok_or_else(||
            TradeFactoryError::SelectionError("No far strikes available for strangle".to_string()))?;
        if far_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("Strangle requires 2 wing strikes".to_string()));
        }

        let put_strike = far_strikes[0];
        let call_strike = far_strikes[1];

        // 7. Create option legs
        let call_leg = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            call_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let put_leg = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            put_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        // 8. Construct and validate strangle
        // Note: Direction is handled at the trade execution level (which legs are bought vs sold)
        cs_domain::entities::Strangle::new(call_leg, put_leg)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Strangle construction failed: {}", e)))
    }

    async fn create_butterfly(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &cs_domain::value_objects::MultiLegStrategyConfig,
    ) -> Result<cs_domain::entities::Butterfly, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Compute DTE range
        let as_of_date = as_of.naive_utc().date();
        let min_dte = (min_expiration - as_of_date).num_days() as i32;
        let max_dte = min_dte + 15;

        // 5. Use selector to build butterfly (2x ATM + wings)
        let selection = self.selector.select_multi_leg(&spot_price, &surface, config, min_dte, max_dte)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Butterfly selection failed: {}", e)))?;

        // 6. Extract center strikes (should be 2 for butterfly)
        if selection.center_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("Butterfly requires 2 center strikes".to_string()));
        }

        let center_strike = selection.center_strikes[0];

        // 7. Extract wing strikes
        let far_strikes = selection.far_strikes.ok_or_else(||
            TradeFactoryError::SelectionError("No far strikes available for butterfly".to_string()))?;
        if far_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("Butterfly requires 2 wing strikes".to_string()));
        }

        let lower_wing_strike = far_strikes[0];
        let upper_wing_strike = far_strikes[1];

        // 8. Create 4 option legs (short call, short put, long upper call, long lower put)
        let short_call = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            center_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let short_put = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            center_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        let long_upper_call = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            upper_wing_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let long_lower_put = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            lower_wing_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        // 9. Construct and validate butterfly
        cs_domain::entities::Butterfly::new(short_call, short_put, long_upper_call, long_lower_put)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Butterfly construction failed: {}", e)))
    }

    async fn create_condor(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &cs_domain::value_objects::MultiLegStrategyConfig,
    ) -> Result<cs_domain::entities::Condor, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Compute DTE range
        let as_of_date = as_of.naive_utc().date();
        let min_dte = (min_expiration - as_of_date).num_days() as i32;
        let max_dte = min_dte + 15;

        // 5. Use selector to build condor (near straddle + far wings)
        let selection = self.selector.select_multi_leg(&spot_price, &surface, config, min_dte, max_dte)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Condor selection failed: {}", e)))?;

        // 6. Extract center strike (should be 1 for condor)
        let _center_strike = selection.center_strikes.first().cloned()
            .ok_or_else(|| TradeFactoryError::SelectionError("No center strike available".to_string()))?;

        // 7. Extract near and far wing strikes
        let near_strikes = selection.near_strikes.ok_or_else(||
            TradeFactoryError::SelectionError("No near strikes available for condor".to_string()))?;
        if near_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("Condor requires 2 near wing strikes".to_string()));
        }

        let far_strikes = selection.far_strikes.ok_or_else(||
            TradeFactoryError::SelectionError("No far strikes available for condor".to_string()))?;
        if far_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("Condor requires 2 far wing strikes".to_string()));
        }

        let near_put_strike = near_strikes[0];
        let near_call_strike = near_strikes[1];
        let far_lower_put_strike = far_strikes[0];
        let far_upper_call_strike = far_strikes[1];

        // 8. Create 4 option legs (near call, near put, far upper call, far lower put)
        let near_call = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            near_call_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let near_put = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            near_put_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        let far_upper_call = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            far_upper_call_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let far_lower_put = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            far_lower_put_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        // 9. Construct and validate condor
        cs_domain::entities::Condor::new(near_call, near_put, far_upper_call, far_lower_put)
            .map_err(|e| TradeFactoryError::SelectionError(format!("Condor construction failed: {}", e)))
    }

    async fn create_iron_condor(
        &self,
        symbol: &str,
        as_of: DateTime<Utc>,
        min_expiration: NaiveDate,
        config: &cs_domain::value_objects::MultiLegStrategyConfig,
    ) -> Result<cs_domain::entities::IronCondor, TradeFactoryError> {
        // 1. Get option chain
        let chain = self.options_repo
            .get_option_bars_at_time(symbol, as_of)
            .await
            .map_err(|e| TradeFactoryError::DataError(format!("Failed to get option chain: {}", e)))?;

        // 2. Build IV surface
        let surface = crate::iv_surface_builder::build_iv_surface_minute_aligned(
            &chain,
            &*self.equity_repo,
            symbol,
        )
        .await
        .ok_or_else(|| TradeFactoryError::SelectionError("Failed to build IV surface".to_string()))?;

        // 3. Extract spot price
        let spot_price = SpotPrice::new(
            Decimal::try_from(surface.spot_price())
                .map_err(|_| TradeFactoryError::DataError("Invalid spot price".to_string()))?,
            as_of,
        );

        // 4. Compute DTE range
        let as_of_date = as_of.naive_utc().date();
        let min_dte = (min_expiration - as_of_date).num_days() as i32;
        let max_dte = min_dte + 15;

        // 5. Use selector to build iron condor (near spread + far wings)
        let selection = self.selector.select_multi_leg(&spot_price, &surface, config, min_dte, max_dte)
            .map_err(|e| TradeFactoryError::SelectionError(format!("IronCondor selection failed: {}", e)))?;

        // 6. Extract near and far wing strikes
        let near_strikes = selection.near_strikes.ok_or_else(||
            TradeFactoryError::SelectionError("No near strikes available for iron condor".to_string()))?;
        if near_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("IronCondor requires 2 near strikes".to_string()));
        }

        let far_strikes = selection.far_strikes.ok_or_else(||
            TradeFactoryError::SelectionError("No far strikes available for iron condor".to_string()))?;
        if far_strikes.len() < 2 {
            return Err(TradeFactoryError::SelectionError("IronCondor requires 2 far strikes".to_string()));
        }

        let near_put_strike = near_strikes[0];
        let near_call_strike = near_strikes[1];
        let far_lower_put_strike = far_strikes[0];
        let far_upper_call_strike = far_strikes[1];

        // 7. Create 4 option legs (near call, near put, far upper call, far lower put)
        let near_call = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            near_call_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let near_put = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            near_put_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        let far_upper_call = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            far_upper_call_strike,
            selection.expiration,
            finq_core::OptionType::Call,
        );

        let far_lower_put = cs_domain::entities::OptionLeg::new(
            symbol.to_string(),
            far_lower_put_strike,
            selection.expiration,
            finq_core::OptionType::Put,
        );

        // 8. Construct and validate iron condor
        cs_domain::entities::IronCondor::new(near_call, near_put, far_upper_call, far_lower_put)
            .map_err(|e| TradeFactoryError::SelectionError(format!("IronCondor construction failed: {}", e)))
    }
}
