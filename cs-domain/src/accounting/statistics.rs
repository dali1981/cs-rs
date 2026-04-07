//! Trade Statistics Calculator
//!
//! Provides comprehensive trade statistics including capital-weighted returns,
//! profit factor, and other risk metrics.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::{ReturnBasis, TradeAccounting};

/// Comprehensive trade statistics
///
/// This struct calculates and holds all key performance metrics for a set of trades,
/// including the critical capital-weighted return that properly accounts for position sizing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TradeStatistics {
    // === Basic Counts ===
    /// Total number of trades
    pub total_trades: usize,
    /// Number of winning trades (P&L > 0)
    pub winning_trades: usize,
    /// Number of losing trades (P&L < 0)
    pub losing_trades: usize,
    /// Number of scratch trades (P&L = 0)
    pub scratch_trades: usize,

    // === Dollar P&L ===
    /// Total realized P&L
    pub total_pnl: Decimal,
    /// Total P&L from options only
    pub total_option_pnl: Decimal,
    /// Total P&L from hedging
    pub total_hedge_pnl: Decimal,
    /// Total transaction costs
    pub total_transaction_costs: Decimal,

    // === Return Metrics ===
    /// Simple mean return (unweighted average of percentage returns)
    /// WARNING: This can be misleading when position sizes vary!
    pub simple_mean_return: f64,

    /// Capital-weighted return (weighted by capital deployed)
    /// THIS IS THE CORRECT METRIC for assessing strategy performance
    pub capital_weighted_return: f64,

    /// Time-weighted return (geometric mean, good for comparing strategies)
    pub time_weighted_return: f64,

    // === Risk Metrics ===
    /// Standard deviation of returns
    pub std_deviation: f64,
    /// Sharpe ratio (annualized, using sqrt(252))
    pub sharpe_ratio: f64,
    /// Maximum drawdown (in dollars)
    pub max_drawdown: Decimal,
    /// Win rate (percentage of profitable trades)
    pub win_rate: f64,

    // === Winner/Loser Analysis ===
    /// Average winning trade (in dollars)
    pub avg_winner_dollars: Decimal,
    /// Average winning trade (percentage return)
    pub avg_winner_pct: f64,
    /// Average losing trade (in dollars)
    pub avg_loser_dollars: Decimal,
    /// Average losing trade (percentage return)
    pub avg_loser_pct: f64,
    /// Profit factor (gross profit / gross loss)
    pub profit_factor: f64,
    /// Payoff ratio (avg winner / avg loser, absolute values)
    pub payoff_ratio: f64,

    // === Capital Efficiency ===
    /// Total capital deployed across all trades
    pub total_capital_deployed: Decimal,
    /// Peak capital required at any point
    pub peak_capital_required: Decimal,
    /// Return on total capital deployed
    pub return_on_total_capital: f64,
    /// Return on peak capital (more conservative)
    pub return_on_peak_capital: f64,
}

impl TradeStatistics {
    /// Calculate statistics from a slice of trade accounting records
    pub fn from_trades(trades: &[TradeAccounting]) -> Self {
        Self::from_trades_with_basis(trades, ReturnBasis::CapitalRequired)
    }

    /// Calculate statistics using an explicit return basis.
    pub fn from_trades_with_basis(trades: &[TradeAccounting], basis: ReturnBasis) -> Self {
        if trades.is_empty() {
            return Self::default();
        }

        // Basic counts
        let total_trades = trades.len();
        let winning_trades = trades.iter().filter(|t| t.realized_pnl > Decimal::ZERO).count();
        let losing_trades = trades.iter().filter(|t| t.realized_pnl < Decimal::ZERO).count();
        let scratch_trades = trades.iter().filter(|t| t.realized_pnl == Decimal::ZERO).count();

        // Dollar P&L
        let total_pnl: Decimal = trades.iter().map(|t| t.realized_pnl).sum();
        let total_option_pnl: Decimal = trades.iter()
            .map(|t| t.realized_pnl - t.hedge_pnl.unwrap_or(Decimal::ZERO))
            .sum();
        let total_hedge_pnl: Decimal = trades.iter()
            .filter_map(|t| t.hedge_pnl)
            .sum();
        let total_transaction_costs: Decimal = trades.iter()
            .map(|t| t.transaction_costs.abs())
            .sum();

        // Basis metrics
        let total_capital_deployed: Decimal = trades.iter()
            .filter_map(|t| t.return_basis_value(basis))
            .sum();

        // Peak basis (simplified - assumes sequential trades)
        let peak_capital_required = trades.iter()
            .filter_map(|t| t.return_basis_value(basis))
            .max()
            .unwrap_or(Decimal::ZERO);

        // Returns
        let returns: Vec<f64> = trades.iter()
            .filter_map(|t| t.return_on_basis(basis))
            .collect();

        // Simple mean return (current behavior - potentially misleading)
        let simple_mean_return = if !returns.is_empty() {
            returns.iter().sum::<f64>() / returns.len() as f64
        } else {
            0.0
        };

        // Capital-weighted return
        let capital_weighted_return = {
            let weighted_sum: f64 = trades.iter()
                .filter_map(|t| {
                    let basis_value = t.return_basis_value(basis)?
                        .to_f64()
                        .unwrap_or(0.0);
                    let ret = t.return_on_basis(basis)?;
                    if basis_value > 0.0 {
                        Some(basis_value * ret)
                    } else {
                        None
                    }
                })
                .sum();
            let total_basis = total_capital_deployed.to_f64().unwrap_or(1.0);
            if total_basis > 0.0 {
                weighted_sum / total_basis
            } else {
                0.0
            }
        };

        // Time-weighted return (geometric mean)
        let time_weighted_return = if !returns.is_empty() {
            let product: f64 = returns.iter()
                .map(|r| 1.0 + r)
                .filter(|x| *x > 0.0) // Avoid log of negative
                .product();
            if product > 0.0 {
                product.powf(1.0 / returns.len() as f64) - 1.0
            } else {
                -1.0 // Total loss
            }
        } else {
            0.0
        };

        // Standard deviation
        let std_deviation = if returns.len() > 1 {
            let mean = capital_weighted_return;
            let variance = returns.iter()
                .map(|r| (r - mean).powi(2))
                .sum::<f64>() / (returns.len() - 1) as f64;
            variance.sqrt()
        } else {
            0.0
        };

        // Sharpe ratio (annualized)
        let sharpe_ratio = if std_deviation > 0.0 {
            capital_weighted_return / std_deviation * 16.0 // sqrt(252) ≈ 16
        } else {
            0.0
        };

        // Win rate
        let win_rate = winning_trades as f64 / total_trades as f64;

        // Winner/Loser analysis
        let winners: Vec<&TradeAccounting> = trades.iter()
            .filter(|t| t.realized_pnl > Decimal::ZERO)
            .collect();
        let losers: Vec<&TradeAccounting> = trades.iter()
            .filter(|t| t.realized_pnl < Decimal::ZERO)
            .collect();

        let avg_winner_dollars = if !winners.is_empty() {
            winners.iter().map(|t| t.realized_pnl).sum::<Decimal>()
                / Decimal::from(winners.len())
        } else {
            Decimal::ZERO
        };

        let avg_loser_dollars = if !losers.is_empty() {
            losers.iter().map(|t| t.realized_pnl).sum::<Decimal>()
                / Decimal::from(losers.len())
        } else {
            Decimal::ZERO
        };

        let avg_winner_pct = if !winners.is_empty() {
            let winner_returns: Vec<f64> = winners.iter()
                .filter_map(|t| t.return_on_basis(basis))
                .collect();
            if winner_returns.is_empty() {
                0.0
            } else {
                winner_returns.iter().sum::<f64>() / winner_returns.len() as f64
            }
        } else {
            0.0
        };

        let avg_loser_pct = if !losers.is_empty() {
            let loser_returns: Vec<f64> = losers.iter()
                .filter_map(|t| t.return_on_basis(basis))
                .collect();
            if loser_returns.is_empty() {
                0.0
            } else {
                loser_returns.iter().sum::<f64>() / loser_returns.len() as f64
            }
        } else {
            0.0
        };

        // Profit factor
        let gross_profit: Decimal = winners.iter().map(|t| t.realized_pnl).sum();
        let gross_loss: Decimal = losers.iter().map(|t| t.realized_pnl.abs()).sum();
        let profit_factor = if !gross_loss.is_zero() {
            (gross_profit / gross_loss).to_f64().unwrap_or(0.0)
        } else if gross_profit > Decimal::ZERO {
            f64::INFINITY
        } else {
            0.0
        };

        // Payoff ratio
        let payoff_ratio = if !avg_loser_dollars.is_zero() {
            (avg_winner_dollars / avg_loser_dollars.abs())
                .to_f64()
                .unwrap_or(0.0)
        } else if avg_winner_dollars > Decimal::ZERO {
            f64::INFINITY
        } else {
            0.0
        };

        // Max drawdown (simple sequential)
        let max_drawdown = Self::calculate_max_drawdown(trades);

        // Capital efficiency (basis-aware)
        let basis_pnl: Decimal = trades.iter()
            .filter(|t| t.return_basis_value(basis).is_some())
            .map(|t| t.realized_pnl)
            .sum();

        let return_on_total_capital = if !total_capital_deployed.is_zero() {
            (basis_pnl / total_capital_deployed).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };

        let return_on_peak_capital = if !peak_capital_required.is_zero() {
            (basis_pnl / peak_capital_required).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };

        Self {
            total_trades,
            winning_trades,
            losing_trades,
            scratch_trades,
            total_pnl,
            total_option_pnl,
            total_hedge_pnl,
            total_transaction_costs,
            simple_mean_return,
            capital_weighted_return,
            time_weighted_return,
            std_deviation,
            sharpe_ratio,
            max_drawdown,
            win_rate,
            avg_winner_dollars,
            avg_winner_pct,
            avg_loser_dollars,
            avg_loser_pct,
            profit_factor,
            payoff_ratio,
            total_capital_deployed,
            peak_capital_required,
            return_on_total_capital,
            return_on_peak_capital,
        }
    }

    /// Calculate maximum drawdown from sequential trades
    fn calculate_max_drawdown(trades: &[TradeAccounting]) -> Decimal {
        let mut peak = Decimal::ZERO;
        let mut max_drawdown = Decimal::ZERO;
        let mut cumulative_pnl = Decimal::ZERO;

        for trade in trades {
            cumulative_pnl += trade.realized_pnl;

            if cumulative_pnl > peak {
                peak = cumulative_pnl;
            }

            let drawdown = peak - cumulative_pnl;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }

        max_drawdown
    }

    /// Get expectancy (expected $ per trade)
    pub fn expectancy(&self) -> Decimal {
        if self.total_trades > 0 {
            self.total_pnl / Decimal::from(self.total_trades)
        } else {
            Decimal::ZERO
        }
    }

    /// Get expectancy ratio (risk-adjusted expectancy)
    /// Expectancy / Average Loser
    pub fn expectancy_ratio(&self) -> f64 {
        if !self.avg_loser_dollars.is_zero() {
            let expectancy = self.expectancy().to_f64().unwrap_or(0.0);
            expectancy / self.avg_loser_dollars.abs().to_f64().unwrap_or(1.0)
        } else {
            0.0
        }
    }
}

/// Builder for calculating statistics from various sources
#[allow(dead_code)]
pub struct TradeStatisticsBuilder {
    trades: Vec<TradeAccounting>,
}

#[allow(dead_code)]
impl TradeStatisticsBuilder {
    pub fn new() -> Self {
        Self { trades: Vec::new() }
    }

    /// Add a trade accounting record
    pub fn add_trade(mut self, trade: TradeAccounting) -> Self {
        self.trades.push(trade);
        self
    }

    /// Add multiple trades
    pub fn add_trades(mut self, trades: impl IntoIterator<Item = TradeAccounting>) -> Self {
        self.trades.extend(trades);
        self
    }

    /// Build the statistics
    pub fn build(self) -> TradeStatistics {
        TradeStatistics::from_trades(&self.trades)
    }
}

impl Default for TradeStatisticsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_trade(entry: Decimal, pnl: Decimal) -> TradeAccounting {
        TradeAccounting::from_pnl(entry, entry + pnl, pnl)
    }

    #[test]
    fn test_capital_weighted_vs_simple() {
        // Reproduce the problem case from the issue
        let trades = vec![
            make_trade(dec!(75), dec!(7.50)),    // +10% return on small position
            make_trade(dec!(170), dec!(-85.00)), // -50% return on large position
            make_trade(dec!(50), dec!(25.00)),   // +50% return on small position
        ];

        let stats = TradeStatistics::from_trades(&trades);

        // Total P&L = 7.50 - 85 + 25 = -52.50
        assert_eq!(stats.total_pnl, dec!(-52.50));

        // Simple mean = (0.10 - 0.50 + 0.50) / 3 ≈ 0.033 (3.33%)
        assert!((stats.simple_mean_return - 0.0333).abs() < 0.01);

        // Capital-weighted return should be negative (matches total P&L direction)
        // = (75*0.10 + 170*(-0.50) + 50*0.50) / (75 + 170 + 50)
        // = (7.5 - 85 + 25) / 295
        // = -52.5 / 295
        // ≈ -0.178 (-17.8%)
        assert!(stats.capital_weighted_return < 0.0);
        assert!((stats.capital_weighted_return - (-0.178)).abs() < 0.01);

        // This demonstrates the fix: capital_weighted_return is negative
        // while simple_mean_return is positive
        assert!(stats.simple_mean_return > 0.0);
        assert!(stats.capital_weighted_return < 0.0);
    }

    #[test]
    fn test_profit_factor() {
        let trades = vec![
            make_trade(dec!(100), dec!(50)),   // Winner
            make_trade(dec!(100), dec!(30)),   // Winner
            make_trade(dec!(100), dec!(-20)),  // Loser
        ];

        let stats = TradeStatistics::from_trades(&trades);

        // Gross profit = 50 + 30 = 80
        // Gross loss = 20
        // Profit factor = 80 / 20 = 4.0
        assert!((stats.profit_factor - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_win_rate() {
        let trades = vec![
            make_trade(dec!(100), dec!(50)),   // Winner
            make_trade(dec!(100), dec!(30)),   // Winner
            make_trade(dec!(100), dec!(-20)),  // Loser
            make_trade(dec!(100), dec!(-10)),  // Loser
        ];

        let stats = TradeStatistics::from_trades(&trades);

        // Win rate = 2 / 4 = 50%
        assert!((stats.win_rate - 0.50).abs() < 0.01);
    }

    #[test]
    fn test_max_drawdown() {
        let trades = vec![
            make_trade(dec!(100), dec!(50)),   // +50, cumulative: 50, peak: 50
            make_trade(dec!(100), dec!(-30)),  // -30, cumulative: 20, dd: 30
            make_trade(dec!(100), dec!(-40)),  // -40, cumulative: -20, dd: 70
            make_trade(dec!(100), dec!(100)),  // +100, cumulative: 80, peak: 80
            make_trade(dec!(100), dec!(-10)),  // -10, cumulative: 70, dd: 10
        ];

        let stats = TradeStatistics::from_trades(&trades);

        // Max drawdown = 70 (peak 50 to trough -20)
        assert_eq!(stats.max_drawdown, dec!(70));
    }

    #[test]
    fn test_empty_trades() {
        let stats = TradeStatistics::from_trades(&[]);

        assert_eq!(stats.total_trades, 0);
        assert_eq!(stats.total_pnl, Decimal::ZERO);
        assert_eq!(stats.capital_weighted_return, 0.0);
    }
}
