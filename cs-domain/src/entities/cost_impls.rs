//! HasTradingCost and ApplyCosts trait implementations for all trade result types

use rust_decimal::Decimal;
use crate::entities::{
    CalendarSpreadResult, IronButterflyResult, StraddleResult,
    CalendarStraddleResult, StrangleResult, ButterflyResult,
    CondorResult, IronCondorResult, CostSummary,
};
use crate::trading_costs::{HasTradingCost, ApplyCosts, TradingCost};

// ============================================================================
// HasTradingCost implementations
// ============================================================================

impl HasTradingCost for CalendarSpreadResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for IronButterflyResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for StraddleResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for CalendarStraddleResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for StrangleResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for ButterflyResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for CondorResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

impl HasTradingCost for IronCondorResult {
    fn cost_summary(&self) -> Option<&CostSummary> {
        self.cost_summary.as_ref()
    }
}

// ============================================================================
// ApplyCosts implementations
// ============================================================================

impl ApplyCosts for CalendarSpreadResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for IronButterflyResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for StraddleResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for CalendarStraddleResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for StrangleResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for ButterflyResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for CondorResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}

impl ApplyCosts for IronCondorResult {
    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn apply_costs(&mut self, costs: TradingCost) {
        if costs.total > Decimal::ZERO {
            let gross_pnl = self.pnl;
            self.pnl = gross_pnl - costs.total;
            self.cost_summary = Some(CostSummary::new(costs, gross_pnl));
        }
    }
}
