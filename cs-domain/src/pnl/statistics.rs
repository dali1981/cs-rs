//! Strategy-level PnL statistics
//!
//! Implements section 6 of the PnL computation spec.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::TradePnlRecord;

/// Annualization factor: sqrt(252 trading days)
const SQRT_252: f64 = 15.874507866387544; // sqrt(252)

/// Strategy-level PnL statistics computed from a collection of trades.
///
/// All metrics use daily-normalized returns for cross-strategy comparability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnlStatistics {
    /// Number of trades
    pub trade_count: usize,

    /// Total PnL across all trades (sum of Total_PnL_i)
    pub total_pnl: Decimal,

    /// Total capital deployed (sum of peak_capital_i)
    pub total_capital: Decimal,

    /// Mean of daily-normalized returns
    pub mean_daily_return: f64,

    /// Standard deviation of daily-normalized returns
    pub std_daily_return: f64,

    /// Annualized Sharpe ratio (spec section 6)
    /// `Sharpe = mean(r_i_daily) / std(r_i_daily) × sqrt(252)`
    pub sharpe_ratio: f64,

    /// Mean hedge cost ratio across trades
    pub mean_hedge_cost_ratio: f64,

    /// Number of trades with excessive hedge costs (>30%)
    pub trades_with_excessive_hedge_costs: usize,

    /// Win rate (proportion of trades with positive total PnL)
    pub win_rate: f64,

    /// Average trade duration in days
    pub avg_duration_days: f64,
}

impl PnlStatistics {
    /// Compute statistics from a slice of trade records.
    ///
    /// Returns `None` if the slice is empty.
    pub fn from_records(records: &[TradePnlRecord]) -> Option<Self> {
        if records.is_empty() {
            return None;
        }

        let n = records.len();

        // Collect daily returns
        let daily_returns: Vec<f64> = records.iter().map(|r| r.daily_return()).collect();

        // Basic aggregates
        let total_pnl: Decimal = records.iter().map(|r| r.total_pnl()).sum();
        let total_capital: Decimal = records.iter().map(|r| r.peak_capital).sum();

        // Mean daily return
        let mean_daily_return = daily_returns.iter().sum::<f64>() / n as f64;

        // Standard deviation of daily returns (sample std dev, n-1)
        let std_daily_return = if n > 1 {
            let variance: f64 = daily_returns
                .iter()
                .map(|r| (r - mean_daily_return).powi(2))
                .sum::<f64>()
                / (n - 1) as f64;
            variance.sqrt()
        } else {
            0.0
        };

        // Sharpe ratio (annualized)
        let sharpe_ratio = if std_daily_return > 0.0 {
            (mean_daily_return / std_daily_return) * SQRT_252
        } else {
            0.0
        };

        // Hedge cost metrics
        let hedge_cost_ratios: Vec<f64> = records.iter().map(|r| r.hedge_cost_ratio()).collect();
        let mean_hedge_cost_ratio = hedge_cost_ratios.iter().sum::<f64>() / n as f64;
        let trades_with_excessive_hedge_costs =
            records.iter().filter(|r| r.has_excessive_hedge_costs()).count();

        // Win rate
        let winners = records.iter().filter(|r| r.total_pnl() > Decimal::ZERO).count();
        let win_rate = winners as f64 / n as f64;

        // Average duration
        let total_days: i64 = records.iter().map(|r| r.duration_days).sum();
        let avg_duration_days = total_days as f64 / n as f64;

        Some(Self {
            trade_count: n,
            total_pnl,
            total_capital,
            mean_daily_return,
            std_daily_return,
            sharpe_ratio,
            mean_hedge_cost_ratio,
            trades_with_excessive_hedge_costs,
            win_rate,
            avg_duration_days,
        })
    }

    /// Capital-weighted return (alternative to mean daily return).
    ///
    /// This weights each trade's return by its capital deployed.
    pub fn capital_weighted_return(&self, records: &[TradePnlRecord]) -> f64 {
        if records.is_empty() {
            return 0.0;
        }

        let weighted_sum: f64 = records
            .iter()
            .map(|r| {
                let capital: f64 = r.peak_capital.try_into().unwrap_or(0.0);
                let daily_return = r.daily_return();
                capital * daily_return
            })
            .sum();

        let total_capital: f64 = records
            .iter()
            .map(|r| r.peak_capital.try_into().unwrap_or(0.0))
            .sum();

        if total_capital > 0.0 {
            weighted_sum / total_capital
        } else {
            0.0
        }
    }

    /// Check if strategy has problematic hedge cost friction.
    ///
    /// Returns true if mean hedge cost ratio > 30% or >20% of trades have excessive costs.
    pub fn has_hedge_cost_problem(&self) -> bool {
        self.mean_hedge_cost_ratio > 0.30
            || (self.trade_count > 0
                && self.trades_with_excessive_hedge_costs as f64 / self.trade_count as f64 > 0.20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_record(option_pnl: Decimal, peak_capital: Decimal, duration_days: i64) -> TradePnlRecord {
        TradePnlRecord::new(
            peak_capital, // option_premium = peak_capital for simplicity
            option_pnl,
            Decimal::ZERO, // no hedge pnl
            Decimal::ZERO, // no hedge costs
            peak_capital,
            duration_days,
        )
    }

    #[test]
    fn test_statistics_basic() {
        let records = vec![
            make_record(dec!(10), dec!(100), 5),   // +10% over 5 days
            make_record(dec!(20), dec!(100), 10),  // +20% over 10 days
            make_record(dec!(-5), dec!(100), 5),   // -5% over 5 days
        ];

        let stats = PnlStatistics::from_records(&records).unwrap();

        assert_eq!(stats.trade_count, 3);
        assert_eq!(stats.total_pnl, dec!(25)); // 10 + 20 - 5
        assert_eq!(stats.total_capital, dec!(300));
        assert!((stats.win_rate - 0.6667).abs() < 0.01); // 2/3 winners
        assert!((stats.avg_duration_days - 6.667).abs() < 0.01);
    }

    #[test]
    fn test_sharpe_positive() {
        // All positive returns should give positive Sharpe
        let records = vec![
            make_record(dec!(10), dec!(100), 10),
            make_record(dec!(15), dec!(100), 10),
            make_record(dec!(12), dec!(100), 10),
        ];

        let stats = PnlStatistics::from_records(&records).unwrap();

        assert!(stats.sharpe_ratio > 0.0);
        assert!(stats.mean_daily_return > 0.0);
    }

    #[test]
    fn test_sharpe_negative() {
        // All negative returns should give negative Sharpe
        let records = vec![
            make_record(dec!(-10), dec!(100), 10),
            make_record(dec!(-15), dec!(100), 10),
            make_record(dec!(-12), dec!(100), 10),
        ];

        let stats = PnlStatistics::from_records(&records).unwrap();

        assert!(stats.sharpe_ratio < 0.0);
        assert!(stats.mean_daily_return < 0.0);
    }

    #[test]
    fn test_hedge_cost_tracking() {
        let records = vec![
            TradePnlRecord::new(
                dec!(100),
                dec!(10),
                dec!(0),
                dec!(40), // 40% hedge cost - excessive!
                dec!(150),
                5,
            ),
            TradePnlRecord::new(
                dec!(100),
                dec!(15),
                dec!(0),
                dec!(10), // 10% hedge cost - ok
                dec!(120),
                5,
            ),
        ];

        let stats = PnlStatistics::from_records(&records).unwrap();

        assert!((stats.mean_hedge_cost_ratio - 0.25).abs() < 0.01); // (40+10)/(100+100) avg
        assert_eq!(stats.trades_with_excessive_hedge_costs, 1);
    }

    #[test]
    fn test_empty_records() {
        let records: Vec<TradePnlRecord> = vec![];
        assert!(PnlStatistics::from_records(&records).is_none());
    }

    #[test]
    fn test_single_trade() {
        let records = vec![make_record(dec!(10), dec!(100), 5)];

        let stats = PnlStatistics::from_records(&records).unwrap();

        assert_eq!(stats.trade_count, 1);
        // With single trade, std dev is 0, so Sharpe is 0
        assert_eq!(stats.sharpe_ratio, 0.0);
        assert!(stats.mean_daily_return > 0.0);
    }

    #[test]
    fn test_capital_weighted_return() {
        // Trade A: $75 capital, +10% daily return
        // Trade B: $170 capital, -50% daily return (for simplicity, 1-day trades)
        // Trade C: $50 capital, +50% daily return
        let records = vec![
            TradePnlRecord::unhedged(dec!(75), dec!(7.5), 1),   // +10%
            TradePnlRecord::unhedged(dec!(170), dec!(-85), 1),  // -50%
            TradePnlRecord::unhedged(dec!(50), dec!(25), 1),    // +50%
        ];

        let stats = PnlStatistics::from_records(&records).unwrap();

        // Simple mean: (10 - 50 + 50) / 3 = 3.33% (misleading - positive!)
        // Capital-weighted: (75*0.10 + 170*(-0.50) + 50*0.50) / 295 = -52.5/295 = -17.8%
        let cw_return = stats.capital_weighted_return(&records);

        // The capital-weighted return should be negative (reflects true economics)
        assert!(cw_return < 0.0);
        assert!((cw_return - (-0.178)).abs() < 0.01);
    }
}
