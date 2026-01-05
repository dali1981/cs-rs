//! Composite trade abstraction for multi-leg option strategies

use crate::entities::OptionLeg;

/// Position direction for a leg
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegPosition {
    Long,   // +1: bought, profit when price rises
    Short,  // -1: sold, profit when price falls
}

impl LegPosition {
    /// Returns the sign as f64 (+1.0 or -1.0)
    pub fn sign(&self) -> f64 {
        match self {
            LegPosition::Long => 1.0,
            LegPosition::Short => -1.0,
        }
    }

    /// Returns the sign as Decimal (+1 or -1)
    pub fn sign_decimal(&self) -> rust_decimal::Decimal {
        match self {
            LegPosition::Long => rust_decimal::Decimal::ONE,
            LegPosition::Short => rust_decimal::Decimal::NEGATIVE_ONE,
        }
    }
}

/// Trait for multi-leg option strategies
///
/// Implementing this trait enables:
/// - Generic pricing (sum leg prices with position signs)
/// - Generic Greeks (sum leg Greeks with position signs)
/// - Generic hedging (net delta/gamma from legs)
pub trait CompositeTrade: Sized + Send + Sync {
    /// Returns all legs with their position (long/short)
    fn legs(&self) -> Vec<(&OptionLeg, LegPosition)>;

    /// Symbol (derived from first leg by default)
    fn symbol(&self) -> &str {
        self.legs().first().map(|(leg, _)| leg.symbol.as_str()).unwrap_or("")
    }

    /// Number of legs
    fn leg_count(&self) -> usize {
        self.legs().len()
    }
}

/// IV information extracted from composite pricing
///
/// Automatically adapts to trade structure:
/// - Non-calendar trades: single IV (average across legs)
/// - Calendar trades: separate short/long IV with ratio
#[derive(Debug, Clone, Copy)]
pub struct CompositeIV {
    /// Primary IV (avg for non-calendars, short leg IV for calendars)
    pub primary: f64,
    /// IV ratio for calendars (short/long), None for non-calendars
    pub ratio: Option<f64>,
    /// Full breakdown by expiration (short_iv, long_iv) for calendars
    pub by_expiration: Option<(f64, f64)>,
}

impl CompositeIV {
    /// Create from non-calendar trade (single IV)
    pub fn single(iv: f64) -> Self {
        Self {
            primary: iv,
            ratio: None,
            by_expiration: None,
        }
    }

    /// Create from calendar trade (short/long IV)
    pub fn calendar(short_iv: f64, long_iv: f64) -> Self {
        Self {
            primary: short_iv,  // Short = earnings-affected leg
            ratio: Some(short_iv / long_iv),
            by_expiration: Some((short_iv, long_iv)),
        }
    }

    /// Calculate change between entry and exit IV
    pub fn change(&self, exit: &CompositeIV) -> CompositeIVChange {
        CompositeIVChange {
            primary_change: (exit.primary - self.primary) / self.primary * 100.0,
            ratio_change: match (self.ratio, exit.ratio) {
                (Some(entry), Some(exit)) => Some((exit - entry) / entry * 100.0),
                _ => None,
            },
        }
    }
}

/// IV change metrics between entry and exit
#[derive(Debug, Clone, Copy)]
pub struct CompositeIVChange {
    /// Primary IV change as percentage
    pub primary_change: f64,
    /// IV ratio change as percentage (calendars only)
    pub ratio_change: Option<f64>,
}
