//! RollableTrade implementations for core trade types

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::entities::*;
use crate::trade::{RollableTrade, TradeResult, TradeConstructionError, TradeTypeId};
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

impl TradeTypeId for Straddle {
    fn type_id(&self) -> &'static str {
        "straddle"
    }
}

impl TradeResult for StraddleResult {
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

    fn hedge_pnl(&self) -> Option<Decimal> {
        self.hedge_pnl
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

impl TradeTypeId for CalendarSpread {
    fn type_id(&self) -> &'static str {
        "calendar_spread"
    }
}

impl TradeResult for CalendarSpreadResult {
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

    fn hedge_pnl(&self) -> Option<Decimal> {
        None  // Calendar spreads typically aren't hedged
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
}
