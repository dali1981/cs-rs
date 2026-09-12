//! Trading campaign and session scheduling
//!
//! A campaign defines the trading intent for one symbol.
//! Sessions are the atomic execution units generated from campaigns.

mod campaign;
mod period_policy;
mod schedule;
mod session;

pub use campaign::TradingCampaign;
pub use period_policy::PeriodPolicy;
pub use schedule::SessionSchedule;
pub use session::{EarningsTimingType, SessionAction, SessionContext, TradingSession};
