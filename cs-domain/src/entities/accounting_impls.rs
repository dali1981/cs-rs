//! HasAccounting implementations for trade result types
//!
//! These implementations allow trade results to provide proper capital
//! accounting data for capital-weighted return calculations.

use rust_decimal::Decimal;

use crate::accounting::{CapitalRequirement, HasAccounting};
use crate::entities::{
    CalendarSpreadResult, CalendarStraddleResult, IronButterflyDirection, IronButterflyResult,
    StraddleResult,
};

impl HasAccounting for StraddleResult {
    fn capital_required(&self) -> CapitalRequirement {
        // Use premium magnitude as the capital basis for now.
        CapitalRequirement::for_premium_basis(self.entry_debit)
    }

    fn entry_cash_flow(&self) -> Decimal {
        // Signed cash flow: pay debit (negative), receive credit (positive).
        -self.entry_debit
    }

    fn exit_cash_flow(&self) -> Decimal {
        self.exit_credit
    }

    fn realized_pnl(&self) -> Decimal {
        // pnl is ALREADY per-contract (multiplied by 100 in execution)
        // Use total P&L with hedge if available, otherwise just option P&L
        self.total_pnl_with_hedge.unwrap_or(self.pnl)
    }

    fn hedge_pnl(&self) -> Option<Decimal> {
        // hedge_pnl is ALREADY per-contract
        self.hedge_pnl
    }

    fn hedge_capital(&self) -> Option<Decimal> {
        // Estimate hedge capital from position if available
        self.hedge_position.as_ref().map(|pos| {
            // Use peak shares * average price as rough capital estimate
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            // Assume 50% margin for Reg-T
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        })
    }
}

impl HasAccounting for CalendarSpreadResult {
    fn capital_required(&self) -> CapitalRequirement {
        CapitalRequirement::for_premium_basis(self.entry_cost)
    }

    fn entry_cash_flow(&self) -> Decimal {
        -self.entry_cost
    }

    fn exit_cash_flow(&self) -> Decimal {
        self.exit_value
    }

    fn realized_pnl(&self) -> Decimal {
        // pnl is ALREADY per-contract (multiplied by 100 in execution)
        self.total_pnl_with_hedge.unwrap_or(self.pnl)
    }

    fn hedge_pnl(&self) -> Option<Decimal> {
        // hedge_pnl is ALREADY per-contract
        self.hedge_pnl
    }

    fn hedge_capital(&self) -> Option<Decimal> {
        self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        })
    }
}

impl HasAccounting for IronButterflyResult {
    fn capital_required(&self) -> CapitalRequirement {
        // Use max loss when available (short); otherwise use debit magnitude (long).
        match self.direction {
            IronButterflyDirection::Short => {
                CapitalRequirement::for_credit(self.entry_credit.abs(), self.max_loss.abs())
            }
            IronButterflyDirection::Long => CapitalRequirement::for_debit(self.entry_credit.abs()),
        }
    }

    fn entry_cash_flow(&self) -> Decimal {
        // entry_credit is signed: positive credit for short, negative debit for long.
        self.entry_credit
    }

    fn exit_cash_flow(&self) -> Decimal {
        let magnitude = self.exit_cost.abs();
        match self.direction {
            IronButterflyDirection::Short => -magnitude,
            IronButterflyDirection::Long => magnitude,
        }
    }

    fn realized_pnl(&self) -> Decimal {
        // pnl is ALREADY per-contract (multiplied by 100 in execution)
        self.total_pnl_with_hedge.unwrap_or(self.pnl)
    }

    fn max_loss(&self) -> Option<Decimal> {
        Some(self.max_loss.abs())
    }

    fn hedge_pnl(&self) -> Option<Decimal> {
        // hedge_pnl is ALREADY per-contract
        self.hedge_pnl
    }

    fn hedge_capital(&self) -> Option<Decimal> {
        self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        })
    }
}

impl HasAccounting for CalendarStraddleResult {
    fn capital_required(&self) -> CapitalRequirement {
        CapitalRequirement::for_premium_basis(self.entry_cost)
    }

    fn entry_cash_flow(&self) -> Decimal {
        -self.entry_cost
    }

    fn exit_cash_flow(&self) -> Decimal {
        self.exit_value
    }

    fn realized_pnl(&self) -> Decimal {
        // pnl is ALREADY per-contract (multiplied by 100 in execution)
        self.total_pnl_with_hedge.unwrap_or(self.pnl)
    }

    fn hedge_pnl(&self) -> Option<Decimal> {
        // hedge_pnl is ALREADY per-contract
        self.hedge_pnl
    }

    fn hedge_capital(&self) -> Option<Decimal> {
        self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // Helper to create minimal test results
    fn make_straddle_result(entry_debit: Decimal, pnl: Decimal) -> StraddleResult {
        use chrono::{TimeZone, Utc};
        use crate::value_objects::{EarningsTime, Strike};
        use crate::entities::PricingSource;

        StraddleResult {
            symbol: "TEST".to_string(),
            earnings_date: None,
            earnings_time: None,
            strike: Strike::new(dec!(100)).unwrap(),
            expiration: chrono::NaiveDate::from_ymd_opt(2025, 1, 17).unwrap(),
            entry_time: Utc.with_ymd_and_hms(2025, 1, 10, 14, 30, 0).unwrap(),
            call_entry_price: entry_debit / dec!(2),
            put_entry_price: entry_debit / dec!(2),
            entry_debit,
            exit_time: Utc.with_ymd_and_hms(2025, 1, 15, 16, 0, 0).unwrap(),
            call_exit_price: (entry_debit + pnl) / dec!(2),
            put_exit_price: (entry_debit + pnl) / dec!(2),
            exit_credit: entry_debit + pnl,
            entry_surface_time: None,
            exit_surface_time: None,
            exit_pricing_method: PricingSource::Market,
            pnl,
            pnl_pct: (pnl / entry_debit) * dec!(100),
            net_delta: None,
            net_gamma: None,
            net_theta: None,
            net_vega: None,
            iv_entry: None,
            iv_exit: None,
            iv_change: None,
            delta_pnl: None,
            gamma_pnl: None,
            theta_pnl: None,
            vega_pnl: None,
            unexplained_pnl: None,
            spot_at_entry: 100.0,
            spot_at_exit: 100.0,
            spot_move: 0.0,
            spot_move_pct: 0.0,
            expected_move_pct: None,
            success: true,
            failure_reason: None,
            hedge_position: None,
            hedge_pnl: None,
            total_pnl_with_hedge: None,
            position_attribution: None,
            cost_summary: None,
            direction: None,
            bpr_timeline: None,
        }
    }

    #[test]
    fn test_straddle_capital_required() {
        // entry_debit is ALREADY per-contract (e.g., $250 for a $2.50 per-share debit)
        let result = make_straddle_result(dec!(250), dec!(50));

        // Capital is the entry_debit directly (already per-contract)
        assert_eq!(result.capital_required().initial_requirement, dec!(250));
    }

    #[test]
    fn test_straddle_return_on_capital() {
        // Values are already per-contract
        let result = make_straddle_result(dec!(250), dec!(50));

        // Return = $50 / $250 = 20%
        let roc = result.return_on_capital();
        assert!((roc - 0.20).abs() < 0.01);
    }

    #[test]
    fn test_straddle_to_accounting() {
        // Values are already per-contract
        let result = make_straddle_result(dec!(250), dec!(50));
        let accounting = result.to_accounting();

        assert_eq!(accounting.capital_required.initial_requirement, dec!(250));
        assert_eq!(accounting.realized_pnl, dec!(50));
        assert!((accounting.return_on_capital - 0.20).abs() < 0.01);
    }
}
