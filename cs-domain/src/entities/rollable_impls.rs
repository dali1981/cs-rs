//! RollableTrade implementations for core trade types

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::entities::*;
use crate::trade::{RollableTrade, TradeResult, TradeConstructionError, CompositeIV};
use crate::ports::TradeFactory;

// ============================================================================
// LongStraddle
// ============================================================================

#[async_trait]
impl RollableTrade for LongStraddle {
    type Result = StraddleResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        factory
            .create_atm_straddle(symbol, dt, min_expiration)
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.expiration()
    }

    fn strike(&self) -> Decimal {
        self.strike().value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}

impl TradeResult for StraddleResult {
    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn entry_cost(&self) -> Decimal {
        self.entry_debit
    }

    fn exit_value(&self) -> Decimal {
        self.exit_credit
    }

    fn success(&self) -> bool {
        self.success
    }

    fn entry_time(&self) -> DateTime<Utc> {
        self.entry_time
    }

    fn exit_time(&self) -> DateTime<Utc> {
        self.exit_time
    }

    fn spot_at_entry(&self) -> f64 {
        self.spot_at_entry
    }

    fn spot_at_exit(&self) -> f64 {
        self.spot_at_exit
    }

    fn net_delta(&self) -> Option<f64> {
        self.net_delta
    }

    fn net_gamma(&self) -> Option<f64> {
        self.net_gamma
    }

    fn entry_iv(&self) -> Option<CompositeIV> {
        self.iv_entry.map(CompositeIV::single)
    }

    fn exit_iv(&self) -> Option<CompositeIV> {
        self.iv_exit.map(CompositeIV::single)
    }

    // iv_change() uses default trait implementation (computed from entry/exit)

    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }

    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }

    fn hedge_position(&self) -> Option<&crate::hedging::HedgePosition> {
        self.hedge_position.as_ref()
    }

    fn apply_hedge_results(
        &mut self,
        position: crate::hedging::HedgePosition,
        hedge_pnl: Decimal,
        total_pnl: Decimal,
        attribution: Option<crate::PositionAttribution>,
    ) {
        self.hedge_position = Some(position);
        self.hedge_pnl = Some(hedge_pnl);
        self.total_pnl_with_hedge = Some(total_pnl);
        self.position_attribution = attribution;
    }
}

// ============================================================================
// CalendarSpread
// ============================================================================

#[async_trait]
impl RollableTrade for CalendarSpread {
    type Result = CalendarSpreadResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        _min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // Use default DTE ranges for calendar spread
        // Short: 0-45 DTE, Long: 45+ DTE (typical pre-earnings calendar)
        // Note: These can be parameterized in future via campaign configuration
        factory
            .create_calendar_spread(
                symbol,
                dt,
                0,      // min_short_dte
                45,     // max_short_dte
                45,     // min_long_dte (must be >= max_short_dte)
                finq_core::OptionType::Call, // Default to Call; can be parameterized
            )
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.short_leg.expiration  // Roll based on short leg
    }

    fn strike(&self) -> Decimal {
        self.short_leg.strike.value()
    }

    fn symbol(&self) -> &str {
        &self.short_leg.symbol
    }
}

impl TradeResult for CalendarSpreadResult {
    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn entry_cost(&self) -> Decimal {
        self.entry_cost
    }

    fn exit_value(&self) -> Decimal {
        self.exit_value
    }

    fn success(&self) -> bool {
        self.success
    }

    fn entry_time(&self) -> DateTime<Utc> {
        self.entry_time
    }

    fn exit_time(&self) -> DateTime<Utc> {
        self.exit_time
    }

    fn spot_at_entry(&self) -> f64 {
        self.spot_at_entry
    }

    fn spot_at_exit(&self) -> f64 {
        self.spot_at_exit
    }

    fn net_delta(&self) -> Option<f64> {
        // Net delta = long_delta - short_delta (since we're short the near leg)
        match (self.long_delta, self.short_delta) {
            (Some(long), Some(short)) => Some(long - short),
            _ => None,
        }
    }

    fn net_gamma(&self) -> Option<f64> {
        // Net gamma = long_gamma - short_gamma
        match (self.long_gamma, self.short_gamma) {
            (Some(long), Some(short)) => Some(long - short),
            _ => None,
        }
    }

    // CalendarSpreadResult doesn't have simple IV fields - it has separate short/long IVs
    // Use default trait implementation (returns None)

    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }

    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }

    fn hedge_position(&self) -> Option<&crate::hedging::HedgePosition> {
        self.hedge_position.as_ref()
    }

    fn apply_hedge_results(
        &mut self,
        position: crate::hedging::HedgePosition,
        hedge_pnl: Decimal,
        total_pnl: Decimal,
        attribution: Option<crate::PositionAttribution>,
    ) {
        self.hedge_position = Some(position);
        self.hedge_pnl = Some(hedge_pnl);
        self.total_pnl_with_hedge = Some(total_pnl);
        self.position_attribution = attribution;
    }
}

// ============================================================================
// IronButterfly
// ============================================================================

#[async_trait]
impl RollableTrade for IronButterfly {
    type Result = IronButterflyResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // Use default wing width of $10 for iron butterfly
        // Note: This can be parameterized in future via campaign configuration
        factory
            .create_iron_butterfly(
                symbol,
                dt,
                min_expiration,
                Decimal::new(10, 0), // $10 wing width
            )
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.expiration()
    }

    fn strike(&self) -> Decimal {
        self.center_strike().value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}

impl TradeResult for IronButterflyResult {
    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn entry_cost(&self) -> Decimal {
        // Iron butterfly receives credit at entry (negative cost)
        -self.entry_credit
    }

    fn exit_value(&self) -> Decimal {
        // Pay to close (negative value)
        -self.exit_cost
    }

    fn success(&self) -> bool {
        self.success
    }

    fn entry_time(&self) -> DateTime<Utc> {
        self.entry_time
    }

    fn exit_time(&self) -> DateTime<Utc> {
        self.exit_time
    }

    fn spot_at_entry(&self) -> f64 {
        self.spot_at_entry
    }

    fn spot_at_exit(&self) -> f64 {
        self.spot_at_exit
    }

    fn net_delta(&self) -> Option<f64> {
        self.net_delta
    }

    fn net_gamma(&self) -> Option<f64> {
        self.net_gamma
    }

    fn entry_iv(&self) -> Option<CompositeIV> {
        self.iv_entry.map(CompositeIV::single)
    }

    fn exit_iv(&self) -> Option<CompositeIV> {
        self.iv_exit.map(CompositeIV::single)
    }

    // iv_change() uses default trait implementation (computed from entry/exit)
    // Note: This replaces the previous iv_crush field usage

    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }

    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }

    fn hedge_position(&self) -> Option<&crate::hedging::HedgePosition> {
        self.hedge_position.as_ref()
    }

    fn apply_hedge_results(
        &mut self,
        position: crate::hedging::HedgePosition,
        hedge_pnl: Decimal,
        total_pnl: Decimal,
        attribution: Option<crate::PositionAttribution>,
    ) {
        self.hedge_position = Some(position);
        self.hedge_pnl = Some(hedge_pnl);
        self.total_pnl_with_hedge = Some(total_pnl);
        self.position_attribution = attribution;
    }
}

// ============================================================================
// CalendarStraddle
// ============================================================================

impl TradeResult for CalendarStraddleResult {
    fn symbol(&self) -> &str {
        &self.symbol
    }

    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn entry_cost(&self) -> Decimal {
        self.entry_cost
    }

    fn exit_value(&self) -> Decimal {
        self.exit_value
    }

    fn success(&self) -> bool {
        self.success
    }

    fn entry_time(&self) -> DateTime<Utc> {
        self.entry_time
    }

    fn exit_time(&self) -> DateTime<Utc> {
        self.exit_time
    }

    fn spot_at_entry(&self) -> f64 {
        self.spot_at_entry
    }

    fn spot_at_exit(&self) -> f64 {
        self.spot_at_exit
    }

    fn net_delta(&self) -> Option<f64> {
        self.net_delta
    }

    fn net_gamma(&self) -> Option<f64> {
        self.net_gamma
    }

    // This result type doesn't have standard IV fields - use default trait implementation (returns None)

    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
    }

    fn total_pnl_with_hedge(&self) -> Option<Decimal> {
        self.total_pnl_with_hedge
    }

    fn hedge_position(&self) -> Option<&crate::hedging::HedgePosition> {
        self.hedge_position.as_ref()
    }

    fn apply_hedge_results(
        &mut self,
        position: crate::hedging::HedgePosition,
        hedge_pnl: Decimal,
        total_pnl: Decimal,
        attribution: Option<crate::PositionAttribution>,
    ) {
        self.hedge_position = Some(position);
        self.hedge_pnl = Some(hedge_pnl);
        self.total_pnl_with_hedge = Some(total_pnl);
        self.position_attribution = attribution;
    }
}

// ============================================================================
// Strangle
// ============================================================================

#[async_trait]
impl RollableTrade for Strangle {
    type Result = StrangleResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // Use 25-delta strangle configuration
        let config = crate::value_objects::MultiLegStrategyConfig::strangle_delta(0.25);
        factory
            .create_strangle(
                symbol,
                dt,
                min_expiration,
                &config,
            )
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.expiration()
    }

    fn strike(&self) -> Decimal {
        // Use call strike as representative strike
        self.call_leg.strike.value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}

// ============================================================================
// Butterfly
// ============================================================================

#[async_trait]
impl RollableTrade for Butterfly {
    type Result = ButterflyResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // Use 25-delta butterfly configuration
        let config = crate::value_objects::MultiLegStrategyConfig::butterfly_delta(0.25);
        factory
            .create_butterfly(
                symbol,
                dt,
                min_expiration,
                &config,
            )
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.short_call.expiration
    }

    fn strike(&self) -> Decimal {
        self.short_call.strike.value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}

// ============================================================================
// Condor
// ============================================================================

#[async_trait]
impl RollableTrade for Condor {
    type Result = CondorResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // Use 10/20-delta condor configuration
        let config = crate::value_objects::MultiLegStrategyConfig::condor_delta(0.10, 0.20);
        factory
            .create_condor(
                symbol,
                dt,
                min_expiration,
                &config,
            )
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.near_call.expiration
    }

    fn strike(&self) -> Decimal {
        self.near_call.strike.value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}

// ============================================================================
// IronCondor
// ============================================================================

#[async_trait]
impl RollableTrade for IronCondor {
    type Result = IronCondorResult;

    async fn create(
        factory: &dyn TradeFactory,
        symbol: &str,
        dt: DateTime<Utc>,
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // Use 10/20-delta iron condor configuration
        let config = crate::value_objects::MultiLegStrategyConfig::iron_condor_delta(0.10, 0.20);
        factory
            .create_iron_condor(
                symbol,
                dt,
                min_expiration,
                &config,
            )
            .await
            .map_err(|e| TradeConstructionError::FactoryError(e.to_string()))
    }

    fn expiration(&self) -> NaiveDate {
        self.near_call.expiration
    }

    fn strike(&self) -> Decimal {
        self.near_call.strike.value()
    }

    fn symbol(&self) -> &str {
        self.symbol()
    }
}
