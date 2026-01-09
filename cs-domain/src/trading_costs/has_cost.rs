//! Trait for trade results that include trading costs

use crate::entities::CostSummary;
use crate::TradingCost;
use rust_decimal::Decimal;

/// Trait for trade results that include trading cost information
///
/// This allows generic code to access cost data across different result types.
pub trait HasTradingCost {
    /// Get the cost summary if available
    fn cost_summary(&self) -> Option<&CostSummary>;

    /// Check if costs were calculated for this trade
    fn has_costs(&self) -> bool {
        self.cost_summary().is_some()
    }

    /// Get gross P&L (before costs) if available
    fn gross_pnl(&self) -> Option<Decimal> {
        self.cost_summary().map(|cs| cs.gross_pnl)
    }

    /// Get total trading costs if available
    fn total_costs(&self) -> Option<Decimal> {
        self.cost_summary().map(|cs| cs.costs.total)
    }
}

/// Trait for applying trading costs to a result (post-processing pattern)
///
/// This trait enables a single point of cost application at the executor level,
/// rather than duplicating cost calculation logic in every strategy implementation.
///
/// # Design
///
/// The `pnl` field in results represents GROSS P&L before costs.
/// After calling `apply_costs()`, the `pnl` field becomes NET P&L (after costs),
/// and `cost_summary` stores both the costs and the original gross P&L.
pub trait ApplyCosts {
    /// Apply trading costs to this result
    ///
    /// This method:
    /// 1. Stores the current `pnl` as gross P&L in `cost_summary`
    /// 2. Subtracts total costs from `pnl` (making it net P&L)
    /// 3. Stores the cost breakdown in `cost_summary`
    ///
    /// If costs are zero, `cost_summary` remains None.
    fn apply_costs(&mut self, costs: TradingCost);

    /// Get the current P&L value (for storing as gross before modification)
    fn pnl(&self) -> Decimal;
}
