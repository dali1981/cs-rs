// cs-domain/src/campaign/session.rs

use chrono::{DateTime, NaiveDate, Utc};
use crate::{EarningsEvent, OptionStrategy, value_objects::{IronButterflyConfig, TradeDirection}};

/// A session is the atomic unit of trading
///
/// One session = one entry-exit period for one symbol.
/// Generated from campaigns, consumed by executors.
#[derive(Debug, Clone)]
pub struct TradingSession {
    /// Symbol to trade
    pub symbol: String,

    /// Strategy type (determines which executor handles this)
    pub strategy: OptionStrategy,

    /// When to enter
    pub entry_datetime: DateTime<Utc>,

    /// When to exit
    pub exit_datetime: DateTime<Utc>,

    /// What action this session represents
    pub action: SessionAction,

    /// Context for understanding this session
    pub context: SessionContext,

    /// Iron butterfly wing configuration (if strategy is IronButterfly)
    pub iron_butterfly_config: Option<IronButterflyConfig>,

    /// Trade direction (defaults to Short)
    pub trade_direction: TradeDirection,
}

/// What action this session represents in a campaign
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    /// First entry of a campaign or period
    OpenNew,

    /// Roll: close current position, open new one
    RollToNext,

    /// Final exit (end of campaign or period)
    CloseOnly,
}

/// Context that explains WHY this session exists
#[derive(Debug, Clone)]
pub enum SessionContext {
    /// Session anchored to an earnings event
    Earnings {
        event: EarningsEvent,
        timing_type: EarningsTimingType,
    },

    /// Session between two earnings dates
    InterEarnings {
        /// Which roll this is (1 = first, 2 = second, etc.)
        roll_number: u16,
        /// Previous earnings date
        earnings_before: NaiveDate,
        /// Next earnings date
        earnings_after: NaiveDate,
    },

    /// Standalone session (no earnings reference)
    Standalone {
        /// Optional description
        note: Option<String>,
    },
}

/// Type of earnings-relative timing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EarningsTimingType {
    /// Enter before earnings, exit before earnings
    PreEarnings,
    /// Enter before earnings, exit after earnings
    CrossEarnings,
    /// Enter after earnings, hold for period
    PostEarnings,
}

impl TradingSession {
    /// Entry date (convenience)
    pub fn entry_date(&self) -> NaiveDate {
        self.entry_datetime.date_naive()
    }

    /// Exit date (convenience)
    pub fn exit_date(&self) -> NaiveDate {
        self.exit_datetime.date_naive()
    }

    /// Duration in trading days (approximate)
    pub fn duration_days(&self) -> i64 {
        (self.exit_datetime - self.entry_datetime).num_days()
    }

    /// Is this an earnings-related session?
    pub fn is_earnings_session(&self) -> bool {
        matches!(self.context, SessionContext::Earnings { .. })
    }
}
