//! Trading cost value object with breakdown
//!
//! Represents the cost of entering or exiting a trade, with full breakdown
//! by component (slippage, commission, market impact).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Represents a trading cost with full breakdown
///
/// Costs are always positive and subtracted from P&L.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TradingCost {
    /// Total cost (always positive, subtracted from P&L)
    pub total: Decimal,

    /// Breakdown by component
    pub breakdown: TradingCostBreakdown,

    /// Which side this cost applies to
    pub side: TradeSide,
}

/// Breakdown of trading costs by category
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TradingCostBreakdown {
    /// Slippage cost (bid-ask spread)
    pub slippage: Decimal,

    /// Commission/fees
    pub commission: Decimal,

    /// Market impact (for large orders)
    pub market_impact: Decimal,

    /// Other costs (regulatory fees, etc.)
    pub other: Decimal,
}

/// Which side of the trade this cost applies to
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeSide {
    #[default]
    Entry,
    Exit,
    /// Combined entry + exit (for round-trip)
    RoundTrip,
}

impl TradingCost {
    /// Create a zero cost
    pub fn zero() -> Self {
        Self::default()
    }

    /// Create a cost with only slippage
    pub fn slippage(amount: Decimal, side: TradeSide) -> Self {
        Self {
            total: amount,
            breakdown: TradingCostBreakdown {
                slippage: amount,
                ..Default::default()
            },
            side,
        }
    }

    /// Create a cost with only commission
    pub fn commission(amount: Decimal, side: TradeSide) -> Self {
        Self {
            total: amount,
            breakdown: TradingCostBreakdown {
                commission: amount,
                ..Default::default()
            },
            side,
        }
    }

    /// Is this a zero cost?
    pub fn is_zero(&self) -> bool {
        self.total.is_zero()
    }
}

impl std::ops::Add for TradingCost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            total: self.total + other.total,
            breakdown: TradingCostBreakdown {
                slippage: self.breakdown.slippage + other.breakdown.slippage,
                commission: self.breakdown.commission + other.breakdown.commission,
                market_impact: self.breakdown.market_impact + other.breakdown.market_impact,
                other: self.breakdown.other + other.breakdown.other,
            },
            side: TradeSide::RoundTrip,
        }
    }
}

impl std::ops::AddAssign for TradingCost {
    fn add_assign(&mut self, other: Self) {
        self.total += other.total;
        self.breakdown.slippage += other.breakdown.slippage;
        self.breakdown.commission += other.breakdown.commission;
        self.breakdown.market_impact += other.breakdown.market_impact;
        self.breakdown.other += other.breakdown.other;
        self.side = TradeSide::RoundTrip;
    }
}

impl std::fmt::Display for TradingCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_zero() {
            write!(f, "$0.00")
        } else {
            write!(f, "${:.2}", self.total)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_trading_cost_add() {
        let entry = TradingCost {
            total: dec!(5.00),
            breakdown: TradingCostBreakdown {
                slippage: dec!(4.00),
                commission: dec!(1.00),
                ..Default::default()
            },
            side: TradeSide::Entry,
        };

        let exit = TradingCost {
            total: dec!(5.00),
            breakdown: TradingCostBreakdown {
                slippage: dec!(4.00),
                commission: dec!(1.00),
                ..Default::default()
            },
            side: TradeSide::Exit,
        };

        let round_trip = entry + exit;
        assert_eq!(round_trip.total, dec!(10.00));
        assert_eq!(round_trip.breakdown.slippage, dec!(8.00));
        assert_eq!(round_trip.breakdown.commission, dec!(2.00));
        assert_eq!(round_trip.side, TradeSide::RoundTrip);
    }

    #[test]
    fn test_trading_cost_display() {
        let cost = TradingCost::slippage(dec!(12.34), TradeSide::Entry);
        assert_eq!(format!("{}", cost), "$12.34");
    }
}
