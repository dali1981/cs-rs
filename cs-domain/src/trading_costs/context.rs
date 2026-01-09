//! Trading context for cost calculations
//!
//! TradingContext provides all the market data needed by cost calculators
//! without coupling to specific pricing implementations.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Context for cost calculations
///
/// Provides market data needed by cost models without coupling to pricing layer.
/// Built from pricing data at the integration boundary.
#[derive(Debug, Clone)]
pub struct TradingContext {
    /// Individual leg contexts (prices and IVs)
    pub legs: Vec<LegContext>,

    /// Number of contracts
    pub num_contracts: u32,

    /// Spot price of underlying
    pub spot_price: f64,

    /// Time of trade
    pub trade_time: DateTime<Utc>,

    /// Underlying symbol
    pub symbol: String,

    /// Trade type (for type-specific costs)
    pub trade_type: TradeType,
}

/// Context for a single leg
#[derive(Debug, Clone)]
pub struct LegContext {
    /// Leg price (per share, before multiplier)
    pub price: Decimal,

    /// Implied volatility for this leg
    pub iv: Option<f64>,

    /// Whether this is a long or short position
    pub is_long: bool,
}

/// Type of trade structure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeType {
    Straddle,
    Strangle,
    CalendarSpread,
    CalendarStraddle,
    IronButterfly,
    IronCondor,
    VerticalSpread,
    /// Single option
    Single,
    /// Custom/unknown structure
    Custom,
}

impl TradingContext {
    /// Create a new trading context
    pub fn new(
        legs: Vec<LegContext>,
        symbol: String,
        spot_price: f64,
        trade_time: DateTime<Utc>,
        trade_type: TradeType,
    ) -> Self {
        Self {
            legs,
            num_contracts: 1,
            spot_price,
            trade_time,
            symbol,
            trade_type,
        }
    }

    /// Set the number of contracts
    pub fn with_contracts(mut self, num_contracts: u32) -> Self {
        self.num_contracts = num_contracts;
        self
    }

    /// Number of legs in the trade
    pub fn num_legs(&self) -> usize {
        self.legs.len()
    }

    /// Average IV across all legs
    pub fn avg_iv(&self) -> Option<f64> {
        let ivs: Vec<f64> = self.legs.iter()
            .filter_map(|leg| leg.iv)
            .collect();

        if ivs.is_empty() {
            None
        } else {
            Some(ivs.iter().sum::<f64>() / ivs.len() as f64)
        }
    }

    /// Total absolute premium across all legs (per share)
    pub fn total_premium(&self) -> Decimal {
        self.legs.iter()
            .map(|leg| leg.price.abs())
            .sum()
    }

    /// Net premium (accounting for long/short positions)
    pub fn net_premium(&self) -> Decimal {
        self.legs.iter()
            .map(|leg| if leg.is_long { leg.price } else { -leg.price })
            .sum()
    }
}

impl LegContext {
    /// Create a long leg context
    pub fn long(price: Decimal, iv: Option<f64>) -> Self {
        Self {
            price,
            iv,
            is_long: true,
        }
    }

    /// Create a short leg context
    pub fn short(price: Decimal, iv: Option<f64>) -> Self {
        Self {
            price,
            iv,
            is_long: false,
        }
    }
}

impl Default for TradeType {
    fn default() -> Self {
        Self::Custom
    }
}

impl std::fmt::Display for TradeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeType::Straddle => write!(f, "Straddle"),
            TradeType::Strangle => write!(f, "Strangle"),
            TradeType::CalendarSpread => write!(f, "Calendar Spread"),
            TradeType::CalendarStraddle => write!(f, "Calendar Straddle"),
            TradeType::IronButterfly => write!(f, "Iron Butterfly"),
            TradeType::IronCondor => write!(f, "Iron Condor"),
            TradeType::VerticalSpread => write!(f, "Vertical Spread"),
            TradeType::Single => write!(f, "Single"),
            TradeType::Custom => write!(f, "Custom"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_context_avg_iv() {
        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(2.50), Some(0.40)),
                LegContext::short(dec!(2.50), Some(0.30)),
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Straddle,
        );

        assert_eq!(ctx.avg_iv(), Some(0.35));
    }

    #[test]
    fn test_context_net_premium() {
        // Straddle: long call + long put
        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(2.50), None),
                LegContext::long(dec!(2.00), None),
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::Straddle,
        );

        // Net premium = pay for both = 2.50 + 2.00 = 4.50
        assert_eq!(ctx.net_premium(), dec!(4.50));
    }

    #[test]
    fn test_calendar_net_premium() {
        // Calendar: long far leg, short near leg
        let ctx = TradingContext::new(
            vec![
                LegContext::long(dec!(3.00), None),  // Long far expiry
                LegContext::short(dec!(2.00), None), // Short near expiry
            ],
            "TEST".to_string(),
            100.0,
            Utc::now(),
            TradeType::CalendarSpread,
        );

        // Net premium = pay 3.00 - receive 2.00 = 1.00 debit
        assert_eq!(ctx.net_premium(), dec!(1.00));
    }
}
