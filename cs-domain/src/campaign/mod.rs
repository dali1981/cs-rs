//! Trading campaign and session scheduling
//!
//! A campaign defines the trading intent for one symbol.
//! Sessions are the atomic execution units generated from campaigns.

mod campaign;
mod session;
mod schedule;
mod period_policy;

pub use campaign::TradingCampaign;
pub use session::{TradingSession, SessionAction, SessionContext, EarningsTimingType};
pub use schedule::SessionSchedule;
pub use period_policy::PeriodPolicy;
