//! Strategy configuration types

use chrono::NaiveDate;
use rust_decimal::Decimal;
use finq_core::OptionType;
use serde::{Serialize, Deserialize};

use crate::{EarningsTime, FailureReason};

/// Trade structure type - defines WHAT to trade
///
/// Used for configuration and dispatching trade selection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TradeStructure {
    CalendarSpread(OptionType),
    Straddle,
    CalendarStraddle,
    IronButterfly { wing_width: Decimal },
}

/// A trade that failed before completion
///
/// Captures failure context for analysis and debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedTrade {
    pub symbol: String,
    pub earnings_date: NaiveDate,
    pub earnings_time: EarningsTime,
    pub trade_structure: TradeStructure,
    pub reason: FailureReason,
    pub phase: String,
    pub details: Option<String>,
}
