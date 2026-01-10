//! CLI wrapper for SpreadType with ValueEnum support

use clap::ValueEnum;
use cs_backtest::SpreadType;
use std::fmt;

/// CLI argument type for spread selection (with clap ValueEnum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SpreadTypeArg {
    /// Calendar spread (default)
    Calendar,
    /// Iron butterfly spread (short: sell ATM straddle, buy wings)
    #[value(name = "iron-butterfly")]
    IronButterfly,
    /// Long iron butterfly (buy ATM straddle, sell wings)
    #[value(name = "long-iron-butterfly")]
    LongIronButterfly,
    /// Long straddle (buy ATM call + put)
    Straddle,
    /// Short straddle (sell ATM call + put)
    #[value(name = "short-straddle")]
    ShortStraddle,
    /// Calendar straddle: short near-term + long far-term straddle
    #[value(name = "calendar-straddle")]
    CalendarStraddle,
    /// Post-earnings straddle: enter day after earnings
    #[value(name = "post-earnings-straddle")]
    PostEarningsStraddle,
}

impl From<SpreadTypeArg> for SpreadType {
    fn from(arg: SpreadTypeArg) -> Self {
        match arg {
            SpreadTypeArg::Calendar => SpreadType::Calendar,
            SpreadTypeArg::IronButterfly => SpreadType::IronButterfly,
            SpreadTypeArg::LongIronButterfly => SpreadType::LongIronButterfly,
            SpreadTypeArg::Straddle => SpreadType::Straddle,
            SpreadTypeArg::ShortStraddle => SpreadType::ShortStraddle,
            SpreadTypeArg::CalendarStraddle => SpreadType::CalendarStraddle,
            SpreadTypeArg::PostEarningsStraddle => SpreadType::PostEarningsStraddle,
        }
    }
}

impl Default for SpreadTypeArg {
    fn default() -> Self {
        SpreadTypeArg::Calendar
    }
}

impl fmt::Display for SpreadTypeArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpreadTypeArg::Calendar => write!(f, "calendar"),
            SpreadTypeArg::IronButterfly => write!(f, "iron-butterfly"),
            SpreadTypeArg::LongIronButterfly => write!(f, "long-iron-butterfly"),
            SpreadTypeArg::Straddle => write!(f, "straddle"),
            SpreadTypeArg::ShortStraddle => write!(f, "short-straddle"),
            SpreadTypeArg::CalendarStraddle => write!(f, "calendar-straddle"),
            SpreadTypeArg::PostEarningsStraddle => write!(f, "post-earnings-straddle"),
        }
    }
}
