//! Trading cost models
//!
//! This module provides various slippage and commission models:
//!
//! - `NoCost`: Null object pattern - zero cost
//! - `FixedPerLegSlippage`: Fixed dollar amount per leg
//! - `PercentageOfPremiumSlippage`: Percentage of premium
//! - `HalfSpreadSlippage`: Half the bid-ask spread
//! - `IVBasedSlippage`: Spread widens with IV
//! - `CommissionModel`: Broker commissions
//! - `CompositeCostCalculator`: Combines multiple models

mod commission;
mod composite;
mod fixed_per_leg;
mod half_spread;
mod iv_based;
mod no_cost;
mod percentage;

pub use commission::CommissionModel;
pub use composite::CompositeCostCalculator;
pub use fixed_per_leg::FixedPerLegSlippage;
pub use half_spread::HalfSpreadSlippage;
pub use iv_based::IVBasedSlippage;
pub use no_cost::NoCost;
pub use percentage::PercentageOfPremiumSlippage;

/// Standard options contract multiplier
const CONTRACT_MULTIPLIER: u32 = 100;
