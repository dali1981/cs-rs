use chrono::{DateTime, NaiveDate, Utc};
use crate::entities::EarningsEvent;

/// Trait for calculating trade entry/exit timing
///
/// This trait abstracts the WHEN dimension - it determines entry and exit
/// dates/times based on the earnings event and timing configuration.
pub trait TradeTiming: Send + Sync {
    /// Entry date for the trade
    fn entry_date(&self, event: &EarningsEvent) -> NaiveDate;

    /// Exit date for the trade
    fn exit_date(&self, event: &EarningsEvent) -> NaiveDate;

    /// Entry datetime (UTC) for the trade
    fn entry_datetime(&self, event: &EarningsEvent) -> DateTime<Utc>;

    /// Exit datetime (UTC) for the trade
    fn exit_datetime(&self, event: &EarningsEvent) -> DateTime<Utc>;
}

pub mod earnings;
pub mod straddle;
pub mod post_earnings;
pub mod trading_calendar;

pub use earnings::EarningsTradeTiming;
pub use straddle::StraddleTradeTiming;
pub use post_earnings::PostEarningsStraddleTiming;
pub use trading_calendar::TradingCalendar;

// Re-export PnL types from cs_analytics for backwards compatibility
pub use cs_analytics::{
    PnLAttribution,
    LegPnL,
    calculate_pnl_attribution,
    calculate_spread_pnl_attribution,
    calculate_option_leg_pnl,
};
