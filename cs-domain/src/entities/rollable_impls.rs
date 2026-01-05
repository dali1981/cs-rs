//! RollableTrade implementations for core trade types

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::entities::*;
use crate::trade::{RollableTrade, TradeResult, TradeConstructionError};
use crate::ports::TradeFactory;

// ============================================================================
// Straddle
// ============================================================================

#[async_trait]
impl RollableTrade for Straddle {
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

    fn iv_entry(&self) -> Option<f64> {
        self.iv_entry
    }

    fn iv_exit(&self) -> Option<f64> {
        self.iv_exit
    }

    fn iv_change(&self) -> Option<f64> {
        self.iv_change
    }

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
        min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // For now, return an error as calendar spread factory method doesn't exist yet
        // This will be implemented in Phase 2.4
        Err(TradeConstructionError::FactoryError(
            "create_calendar_spread not yet implemented in TradeFactory".to_string()
        ))
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
        _factory: &dyn TradeFactory,
        _symbol: &str,
        _dt: DateTime<Utc>,
        _min_expiration: NaiveDate,
    ) -> Result<Self, TradeConstructionError> {
        // For now, return an error as iron butterfly factory method doesn't exist yet
        // This will be implemented in Phase 2.5
        Err(TradeConstructionError::FactoryError(
            "create_iron_butterfly not yet implemented in TradeFactory".to_string()
        ))
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

    fn iv_entry(&self) -> Option<f64> {
        self.iv_entry
    }

    fn iv_exit(&self) -> Option<f64> {
        self.iv_exit
    }

    fn iv_change(&self) -> Option<f64> {
        self.iv_crush  // Iron butterfly uses iv_crush instead of iv_change
    }

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
