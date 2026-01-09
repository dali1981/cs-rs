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

mod no_cost;
mod fixed_per_leg;
mod percentage;
mod half_spread;
mod iv_based;
mod commission;
mod composite;

pub use no_cost::NoCost;
pub use fixed_per_leg::FixedPerLegSlippage;
pub use percentage::PercentageOfPremiumSlippage;
pub use half_spread::HalfSpreadSlippage;
pub use iv_based::IVBasedSlippage;
pub use commission::CommissionModel;
pub use composite::CompositeCostCalculator;

/// Standard options contract multiplier
const CONTRACT_MULTIPLIER: u32 = 100;
