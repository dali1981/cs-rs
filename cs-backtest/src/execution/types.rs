//! Types for generic trade execution

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use cs_domain::EarningsEvent;

/// Configuration for trade validation
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Maximum allowed IV at entry (filters unreliable pricing)
    pub max_entry_iv: Option<f64>,

    /// Minimum entry cost to avoid near-zero pricing
    pub min_entry_cost: Decimal,

    /// Minimum credit for credit spreads (optional)
    pub min_credit: Option<Decimal>,
}

impl ExecutionConfig {
    /// Create config for straddle execution
    pub fn for_straddle(max_entry_iv: Option<f64>) -> Self {
        Self {
            max_entry_iv,
            min_entry_cost: Decimal::new(50, 2), // $0.50 minimum for straddles
            min_credit: None,
        }
    }

    /// Create config for calendar spread execution
    pub fn for_calendar_spread(max_entry_iv: Option<f64>) -> Self {
        Self {
            max_entry_iv,
            min_entry_cost: Decimal::new(5, 2), // $0.05 minimum for calendar spreads
            min_credit: None,
        }
    }
}

/// Context passed to result construction
///
/// Contains all execution data needed to construct a result
#[derive(Debug)]
pub struct ExecutionContext<'a> {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_spot: f64,
    pub exit_spot: f64,
    pub entry_surface_time: Option<DateTime<Utc>>,
    pub exit_surface_time: DateTime<Utc>,
    pub earnings_event: &'a EarningsEvent,
}

impl<'a> ExecutionContext<'a> {
    /// Create a new execution context
    pub fn new(
        entry_time: DateTime<Utc>,
        exit_time: DateTime<Utc>,
        entry_spot: f64,
        exit_spot: f64,
        entry_surface_time: Option<DateTime<Utc>>,
        exit_surface_time: DateTime<Utc>,
        earnings_event: &'a EarningsEvent,
    ) -> Self {
        Self {
            entry_time,
            exit_time,
            entry_spot,
            exit_spot,
            entry_surface_time,
            exit_surface_time,
            earnings_event,
        }
    }
}
