//! Conversion traits for creating TradePnlRecord from trade results

use rust_decimal::Decimal;

use super::TradePnlRecord;
use crate::entities::{
    CalendarSpreadResult, CalendarStraddleResult, IronButterflyResult, StraddleResult,
};
use crate::trading_costs::HasTradingCost;

/// Trait for converting trade results to TradePnlRecord
pub trait ToPnlRecord {
    /// Convert this result to a TradePnlRecord for normalized return computation.
    fn to_pnl_record(&self) -> TradePnlRecord;
}

impl ToPnlRecord for StraddleResult {
    fn to_pnl_record(&self) -> TradePnlRecord {
        let duration_days = (self.exit_time - self.entry_time).num_days();

        // Option premium is the entry debit magnitude
        let option_premium = self.entry_debit.abs();

        // Option P&L (before hedge)
        let option_pnl = self.pnl;

        // Hedge P&L
        let hedge_pnl = self.hedge_pnl.unwrap_or(Decimal::ZERO);

        // Hedge costs from trading costs if available
        let hedge_costs = self.total_costs().unwrap_or(Decimal::ZERO);

        // Peak capital: option premium + hedge capital (if hedging)
        let hedge_capital = self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            // 50% margin for Reg-T
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        }).unwrap_or(Decimal::ZERO);

        let peak_capital = option_premium + hedge_capital;

        TradePnlRecord::new(
            option_premium,
            option_pnl,
            hedge_pnl,
            hedge_costs,
            peak_capital,
            duration_days,
        )
    }
}

impl ToPnlRecord for CalendarSpreadResult {
    fn to_pnl_record(&self) -> TradePnlRecord {
        let duration_days = (self.exit_time - self.entry_time).num_days();

        // Option premium is the absolute entry cost
        let option_premium = self.entry_cost.abs();

        // Option P&L (before hedge)
        let option_pnl = self.pnl;

        // Hedge P&L
        let hedge_pnl = self.hedge_pnl.unwrap_or(Decimal::ZERO);

        // Hedge costs from trading costs if available
        let hedge_costs = self.total_costs().unwrap_or(Decimal::ZERO);

        // Peak capital: option premium + hedge capital (if hedging)
        let hedge_capital = self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        }).unwrap_or(Decimal::ZERO);

        let peak_capital = option_premium + hedge_capital;

        TradePnlRecord::new(
            option_premium,
            option_pnl,
            hedge_pnl,
            hedge_costs,
            peak_capital,
            duration_days,
        )
    }
}

impl ToPnlRecord for IronButterflyResult {
    fn to_pnl_record(&self) -> TradePnlRecord {
        let duration_days = (self.exit_time - self.entry_time).num_days();

        // For credit spreads, use credit received as "premium" basis
        // This is conservative - actual capital at risk depends on wing width
        let option_premium = self.entry_credit.abs();

        // Option P&L (before hedge)
        let option_pnl = self.pnl;

        // Hedge P&L
        let hedge_pnl = self.hedge_pnl.unwrap_or(Decimal::ZERO);

        // Hedge costs
        let hedge_costs = self.total_costs().unwrap_or(Decimal::ZERO);

        // Peak capital
        let hedge_capital = self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        }).unwrap_or(Decimal::ZERO);

        let peak_capital = option_premium + hedge_capital;

        TradePnlRecord::new(
            option_premium,
            option_pnl,
            hedge_pnl,
            hedge_costs,
            peak_capital,
            duration_days,
        )
    }
}

impl ToPnlRecord for CalendarStraddleResult {
    fn to_pnl_record(&self) -> TradePnlRecord {
        let duration_days = (self.exit_time - self.entry_time).num_days();

        // Option premium is the absolute entry cost
        let option_premium = self.entry_cost.abs();

        // Option P&L (before hedge)
        let option_pnl = self.pnl;

        // Hedge P&L
        let hedge_pnl = self.hedge_pnl.unwrap_or(Decimal::ZERO);

        // Hedge costs
        let hedge_costs = self.total_costs().unwrap_or(Decimal::ZERO);

        // Peak capital
        let hedge_capital = self.hedge_position.as_ref().map(|pos| {
            let peak_shares = pos.peak_long_shares.max(pos.peak_short_shares);
            let avg_price = Decimal::try_from(pos.avg_hedge_price).unwrap_or(Decimal::ZERO);
            Decimal::from(peak_shares) * avg_price * Decimal::new(50, 2)
        }).unwrap_or(Decimal::ZERO);

        let peak_capital = option_premium + hedge_capital;

        TradePnlRecord::new(
            option_premium,
            option_pnl,
            hedge_pnl,
            hedge_costs,
            peak_capital,
            duration_days,
        )
    }
}
