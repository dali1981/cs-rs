use crate::{
    ExpirationPolicy,
    value_objects::TradeDirection,
};

/// What option position structure to trade
///
/// This is a simplified, backtest-focused config that specifies the structure,
/// selection method, and direction for positions.
///
/// Note: This is different from TradeStructureConfig which is execution-focused.
/// PositionSpec is configuration, TradeStructureConfig is the runtime structure.
#[derive(Debug, Clone)]
pub struct PositionSpec {
    /// Structure type (Calendar, IronButterfly, Straddle, etc.)
    pub structure: PositionStructure,

    /// Strike selection method
    pub selection: StrikeSelection,

    /// Trade direction (Long or Short)
    pub direction: TradeDirection,

    /// Expiration selection policy
    pub expiration_policy: ExpirationPolicy,
}

/// Position structure types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionStructure {
    /// Calendar spread (single option type, two expirations)
    Calendar,

    /// Iron butterfly (short ATM call+put, long OTM call+put)
    IronButterfly,

    /// Straddle (ATM call + ATM put, same expiration)
    Straddle,

    /// Calendar straddle (straddle at two expirations)
    CalendarStraddle,

    /// Post-earnings straddle (enter after earnings, hold for period)
    PostEarningsStraddle,
}

/// Strike selection methods
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrikeSelection {
    /// At-the-money (closest to spot price)
    ATM,

    /// Fixed delta value
    Delta { target: f64 },

    /// Scan range of deltas for best opportunity
    DeltaScan { min: f64, max: f64, steps: usize },
}

impl Default for PositionSpec {
    fn default() -> Self {
        Self {
            structure: PositionStructure::Calendar,
            selection: StrikeSelection::ATM,
            direction: TradeDirection::Short,
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            },
        }
    }
}

impl PositionSpec {
    /// Create a calendar spread spec
    pub fn calendar() -> Self {
        Self {
            structure: PositionStructure::Calendar,
            ..Default::default()
        }
    }

    /// Create an iron butterfly spec
    pub fn iron_butterfly() -> Self {
        Self {
            structure: PositionStructure::IronButterfly,
            ..Default::default()
        }
    }

    /// Create a straddle spec
    pub fn straddle() -> Self {
        Self {
            structure: PositionStructure::Straddle,
            ..Default::default()
        }
    }

    /// Set selection method
    pub fn with_selection(mut self, selection: StrikeSelection) -> Self {
        self.selection = selection;
        self
    }

    /// Set direction
    pub fn with_direction(mut self, direction: TradeDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set expiration policy
    pub fn with_expiration_policy(mut self, policy: ExpirationPolicy) -> Self {
        self.expiration_policy = policy;
        self
    }
}
