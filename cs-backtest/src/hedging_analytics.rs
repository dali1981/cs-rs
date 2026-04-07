use cs_domain::StraddleResult;
use rust_decimal::Decimal;

/// Compare hedged vs unhedged performance for a single trade
#[derive(Debug, Clone)]
pub struct HedgingComparison {
    pub unhedged_pnl: Decimal,
    pub hedged_pnl: Decimal,
    pub hedge_contribution: Decimal, // hedged_pnl - unhedged_pnl
    pub hedge_cost: Decimal,
    pub num_rehedges: usize,
    pub hedge_efficiency: f64, // hedge_contribution / hedge_cost
}

impl HedgingComparison {
    /// Create comparison from a StraddleResult with hedging data
    pub fn from_result(result: &StraddleResult) -> Option<Self> {
        let hedge_pos = result.hedge_position.as_ref()?;
        let _hedge_pnl = result.hedge_pnl?;
        let total_pnl = result.total_pnl_with_hedge?;

        let unhedged_pnl = result.pnl;
        let hedge_contribution = total_pnl - unhedged_pnl;
        let hedge_cost = hedge_pos.total_cost;

        let hedge_efficiency = if hedge_cost > Decimal::ZERO {
            (hedge_contribution / hedge_cost)
                .try_into()
                .unwrap_or(0.0)
        } else {
            0.0
        };

        Some(Self {
            unhedged_pnl,
            hedged_pnl: total_pnl,
            hedge_contribution,
            hedge_cost,
            num_rehedges: hedge_pos.rehedge_count(),
            hedge_efficiency,
        })
    }
}

/// Aggregate statistics for hedging backtest
#[derive(Debug, Clone)]
pub struct HedgingStats {
    pub total_trades: usize,
    pub hedged_trades: usize,
    pub avg_rehedges_per_trade: f64,
    pub total_hedge_cost: Decimal,
    pub total_hedge_pnl: Decimal,
    pub avg_hedge_efficiency: f64,
    pub hedged_sharpe: Option<f64>,
    pub unhedged_sharpe: Option<f64>,
}

impl HedgingStats {
    /// Compute aggregate statistics from a collection of results
    pub fn from_results(results: &[StraddleResult]) -> Self {
        let total_trades = results.len();
        let mut hedged_trades = 0;
        let mut total_rehedges = 0;
        let mut total_hedge_cost = Decimal::ZERO;
        let mut total_hedge_pnl = Decimal::ZERO;
        let mut sum_efficiency = 0.0;
        let mut hedged_pnls = Vec::new();
        let mut unhedged_pnls = Vec::new();

        for result in results {
            if let Some(comparison) = HedgingComparison::from_result(result) {
                hedged_trades += 1;
                total_rehedges += comparison.num_rehedges;
                total_hedge_cost += comparison.hedge_cost;
                total_hedge_pnl += result.hedge_pnl.unwrap_or(Decimal::ZERO);
                sum_efficiency += comparison.hedge_efficiency;
                hedged_pnls.push(comparison.hedged_pnl);
                unhedged_pnls.push(comparison.unhedged_pnl);
            }
        }

        let avg_rehedges_per_trade = if hedged_trades > 0 {
            total_rehedges as f64 / hedged_trades as f64
        } else {
            0.0
        };

        let avg_hedge_efficiency = if hedged_trades > 0 {
            sum_efficiency / hedged_trades as f64
        } else {
            0.0
        };

        let hedged_sharpe = Self::calculate_sharpe(&hedged_pnls);
        let unhedged_sharpe = Self::calculate_sharpe(&unhedged_pnls);

        Self {
            total_trades,
            hedged_trades,
            avg_rehedges_per_trade,
            total_hedge_cost,
            total_hedge_pnl,
            avg_hedge_efficiency,
            hedged_sharpe,
            unhedged_sharpe,
        }
    }

    /// Calculate Sharpe ratio from P&L series
    fn calculate_sharpe(pnls: &[Decimal]) -> Option<f64> {
        if pnls.len() < 2 {
            return None;
        }

        // Convert to f64
        let pnls_f64: Vec<f64> = pnls
            .iter()
            .filter_map(|d| TryInto::<f64>::try_into(*d).ok())
            .collect();

        if pnls_f64.len() < 2 {
            return None;
        }

        // Calculate mean
        let mean = pnls_f64.iter().sum::<f64>() / pnls_f64.len() as f64;

        // Calculate standard deviation
        let variance = pnls_f64
            .iter()
            .map(|x| {
                let diff = x - mean;
                diff * diff
            })
            .sum::<f64>()
            / pnls_f64.len() as f64;

        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return None;
        }

        // Sharpe ratio (assuming risk-free rate = 0 for simplicity)
        Some(mean / std_dev)
    }
}

impl Default for HedgingStats {
    fn default() -> Self {
        Self {
            total_trades: 0,
            hedged_trades: 0,
            avg_rehedges_per_trade: 0.0,
            total_hedge_cost: Decimal::ZERO,
            total_hedge_pnl: Decimal::ZERO,
            avg_hedge_efficiency: 0.0,
            hedged_sharpe: None,
            unhedged_sharpe: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sharpe_calculation() {
        let pnls = vec![
            Decimal::try_from(100.0).unwrap(),
            Decimal::try_from(200.0).unwrap(),
            Decimal::try_from(-50.0).unwrap(),
            Decimal::try_from(150.0).unwrap(),
        ];

        let sharpe = HedgingStats::calculate_sharpe(&pnls);
        assert!(sharpe.is_some());
        assert!(sharpe.unwrap() > 0.0);
    }

    #[test]
    fn test_sharpe_with_empty_data() {
        let pnls: Vec<Decimal> = vec![];
        let sharpe = HedgingStats::calculate_sharpe(&pnls);
        assert!(sharpe.is_none());
    }
}
