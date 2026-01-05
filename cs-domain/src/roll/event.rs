use chrono::{NaiveDate, DateTime, Utc};
use rust_decimal::Decimal;

/// Record of a roll event
#[derive(Debug, Clone)]
pub struct RollEvent {
    /// When the roll occurred
    pub timestamp: DateTime<Utc>,

    /// Expiration of position being closed
    pub old_expiration: NaiveDate,

    /// Expiration of new position
    pub new_expiration: NaiveDate,

    /// Value received for closing old position
    pub close_value: Decimal,

    /// Cost of opening new position
    pub open_cost: Decimal,

    /// Net credit/debit of roll
    pub net_credit: Decimal,

    /// Spot price at roll time
    pub spot_at_roll: f64,
}

impl RollEvent {
    pub fn new(
        timestamp: DateTime<Utc>,
        old_expiration: NaiveDate,
        new_expiration: NaiveDate,
        close_value: Decimal,
        open_cost: Decimal,
        spot_at_roll: f64,
    ) -> Self {
        Self {
            timestamp,
            old_expiration,
            new_expiration,
            close_value,
            open_cost,
            net_credit: close_value - open_cost,
            spot_at_roll,
        }
    }
}
