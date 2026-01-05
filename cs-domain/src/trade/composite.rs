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
