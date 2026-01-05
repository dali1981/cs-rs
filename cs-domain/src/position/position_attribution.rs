use chrono::NaiveDate;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::PositionSnapshot;

/// Daily P&L attribution breakdown
/// All values computed from daily moves (not cumulative from entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyAttribution {
    pub date: NaiveDate,

    // Daily market data
    pub spot_open: f64,
    pub spot_close: f64,
    pub spot_change: f64,  // Daily spot move
    pub iv_open: f64,
    pub iv_close: f64,
    pub iv_change: f64,    // Daily IV move

    // Position state at start of day (Greeks recomputed daily)
    pub option_delta: f64,  // Position-level (×100)
    pub option_gamma: f64,  // Position-level (×100)
    pub hedge_shares: i32,
    pub net_delta: f64,     // option_delta + hedge_shares

    // P&L components (position-level, in dollars)
    pub gross_delta_pnl: f64,  // option_delta × daily_spot_change
    pub hedge_delta_pnl: f64,  // hedge_shares × daily_spot_change
    pub net_delta_pnl: f64,    // net_delta × daily_spot_change
    pub gamma_pnl: f64,        // 0.5 × gamma × daily_spot_change²
    pub theta_pnl: f64,        // theta (per day, recomputed daily)
    pub vega_pnl: f64,         // vega × daily_iv_change × 100
}

impl DailyAttribution {
    /// Compute daily P&L attribution from start-of-day and end-of-day snapshots
    ///
    /// IMPORTANT: Greeks in start_snapshot must be freshly computed for that day
    /// (not carried forward from entry). This ensures accurate attribution as
    /// delta/gamma/theta/vega evolve with spot, IV, and time-to-expiry.
    ///
    /// # Arguments
    /// * `start_snapshot` - Position state at market open (Greeks recomputed)
    /// * `end_snapshot` - Position state at market close (Greeks recomputed)
    ///
    /// # Returns
    /// Daily attribution with breakdown of P&L by Greek
    pub fn compute(
        start_snapshot: &PositionSnapshot,
        end_snapshot: &PositionSnapshot,
    ) -> Self {
        // Daily spot move (NOT total move from trade entry)
        let spot_change = end_snapshot.spot - start_snapshot.spot;

        // Delta P&L components (using start-of-day Greeks)
        let gross_delta_pnl = start_snapshot.option_greeks.delta * spot_change;
        let hedge_delta_pnl = start_snapshot.hedge_shares as f64 * spot_change;
        let net_delta_pnl = start_snapshot.net_delta * spot_change;

        // Gamma P&L: 0.5 × gamma × (daily_spot_change)²
        // This is the KEY fix: uses daily move squared, not total move squared
        let gamma_pnl = 0.5 * start_snapshot.option_greeks.gamma * spot_change.powi(2);

        // Theta P&L: already expressed per day
        let theta_pnl = start_snapshot.option_greeks.theta;

        // Vega P&L: vega × daily_iv_change × 100
        let iv_change = end_snapshot.iv - start_snapshot.iv;
        let vega_pnl = start_snapshot.option_greeks.vega * iv_change * 100.0;

        Self {
            date: start_snapshot.timestamp.date_naive(),
            spot_open: start_snapshot.spot,
            spot_close: end_snapshot.spot,
            spot_change,
            iv_open: start_snapshot.iv,
            iv_close: end_snapshot.iv,
            iv_change,
            option_delta: start_snapshot.option_greeks.delta,
            option_gamma: start_snapshot.option_greeks.gamma,
            hedge_shares: start_snapshot.hedge_shares,
            net_delta: start_snapshot.net_delta,
            gross_delta_pnl,
            hedge_delta_pnl,
            net_delta_pnl,
            gamma_pnl,
            theta_pnl,
            vega_pnl,
        }
    }
}

/// Aggregated attribution over entire holding period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionAttribution {
    pub daily: Vec<DailyAttribution>,

    // Totals (sum of daily)
    pub total_gross_delta_pnl: Decimal,
    pub total_hedge_delta_pnl: Decimal,
    pub total_net_delta_pnl: Decimal,
    pub total_gamma_pnl: Decimal,
    pub total_theta_pnl: Decimal,
    pub total_vega_pnl: Decimal,
    pub total_unexplained: Decimal,

    // Hedge effectiveness metrics
    pub hedge_efficiency: f64,  // |hedge_delta_pnl| / |gross_delta_pnl| × 100
}

impl PositionAttribution {
    /// Calculate attribution from paired daily snapshots
    ///
    /// Convenience method that computes daily attributions from snapshot pairs
    /// and then aggregates them.
    ///
    /// # Arguments
    /// * `snapshots` - Vec of (start_of_day, end_of_day) snapshot pairs
    /// * `actual_pnl` - Actual realized P&L for unexplained calculation
    ///
    /// # Returns
    /// Aggregated attribution with totals and hedge efficiency
    pub fn from_snapshots(
        snapshots: Vec<(PositionSnapshot, PositionSnapshot)>,
        actual_pnl: Decimal,
    ) -> Self {
        let daily: Vec<DailyAttribution> = snapshots
            .iter()
            .map(|(start, end)| DailyAttribution::compute(start, end))
            .collect();

        Self::from_daily(daily, actual_pnl)
    }

    /// Aggregate daily attributions into period totals
    ///
    /// # Arguments
    /// * `daily` - Vector of daily attributions (one per trading day)
    /// * `actual_pnl` - Actual realized P&L for unexplained calculation
    ///
    /// # Returns
    /// Aggregated attribution with totals and hedge efficiency
    pub fn from_daily(daily: Vec<DailyAttribution>, actual_pnl: Decimal) -> Self {
        let total_gross_delta: f64 = daily.iter().map(|d| d.gross_delta_pnl).sum();
        let total_hedge_delta: f64 = daily.iter().map(|d| d.hedge_delta_pnl).sum();
        let total_net_delta: f64 = daily.iter().map(|d| d.net_delta_pnl).sum();
        let total_gamma: f64 = daily.iter().map(|d| d.gamma_pnl).sum();
        let total_theta: f64 = daily.iter().map(|d| d.theta_pnl).sum();
        let total_vega: f64 = daily.iter().map(|d| d.vega_pnl).sum();

        let explained = total_net_delta + total_gamma + total_theta + total_vega;
        let unexplained = actual_pnl.to_f64().unwrap_or(0.0) - explained;

        // Hedge efficiency: how much of the gross delta move was offset by hedges
        let hedge_efficiency = if total_gross_delta.abs() > 0.01 {
            (total_hedge_delta.abs() / total_gross_delta.abs()) * 100.0
        } else {
            0.0
        };

        Self {
            daily,
            total_gross_delta_pnl: Decimal::try_from(total_gross_delta).unwrap_or_default(),
            total_hedge_delta_pnl: Decimal::try_from(total_hedge_delta).unwrap_or_default(),
            total_net_delta_pnl: Decimal::try_from(total_net_delta).unwrap_or_default(),
            total_gamma_pnl: Decimal::try_from(total_gamma).unwrap_or_default(),
            total_theta_pnl: Decimal::try_from(total_theta).unwrap_or_default(),
            total_vega_pnl: Decimal::try_from(total_vega).unwrap_or_default(),
            total_unexplained: Decimal::try_from(unexplained).unwrap_or_default(),
            hedge_efficiency,
        }
    }

    /// Number of trading days in the attribution period
    pub fn num_days(&self) -> usize {
        self.daily.len()
    }

    /// Average daily P&L for each Greek
    pub fn avg_daily_delta_pnl(&self) -> f64 {
        if self.daily.is_empty() {
            0.0
        } else {
            self.total_net_delta_pnl.to_f64().unwrap_or(0.0) / self.daily.len() as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::position::PositionGreeks;

    #[test]
    fn test_daily_attribution_compute() {
        let start_greeks = PositionGreeks {
            delta: 50.0,
            gamma: 5.0,
            theta: -20.0,
            vega: 30.0,
        };

        let start_snapshot = PositionSnapshot::new(
            Utc::now(),
            100.0,  // spot_open
            0.30,   // iv_open
            start_greeks,
            -30,    // hedge_shares (short 30)
        );

        let end_greeks = PositionGreeks {
            delta: 60.0,  // Delta changed during day
            gamma: 5.5,
            theta: -19.0,
            vega: 32.0,
        };

        let end_snapshot = PositionSnapshot::new(
            Utc::now(),
            102.0,  // spot_close (+2)
            0.32,   // iv_close (+0.02)
            end_greeks,
            -30,
        );

        let attr = DailyAttribution::compute(&start_snapshot, &end_snapshot);

        // Spot change = 102 - 100 = 2
        assert_eq!(attr.spot_change, 2.0);

        // Gross delta P&L = 50 × 2 = 100
        assert_eq!(attr.gross_delta_pnl, 100.0);

        // Hedge delta P&L = -30 × 2 = -60
        assert_eq!(attr.hedge_delta_pnl, -60.0);

        // Net delta P&L = (50 - 30) × 2 = 40
        assert_eq!(attr.net_delta_pnl, 40.0);

        // Gamma P&L = 0.5 × 5 × 2² = 10
        assert_eq!(attr.gamma_pnl, 10.0);

        // Theta P&L = -20 (per day)
        assert_eq!(attr.theta_pnl, -20.0);

        // Vega P&L = 30 × 0.02 × 100 = 60
        assert!((attr.vega_pnl - 60.0).abs() < 0.01);
    }

    #[test]
    fn test_position_attribution_from_daily() {
        // Create two days of attribution
        let day1 = DailyAttribution {
            date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            spot_open: 100.0,
            spot_close: 102.0,
            spot_change: 2.0,
            iv_open: 0.30,
            iv_close: 0.32,
            iv_change: 0.02,
            option_delta: 50.0,
            option_gamma: 5.0,
            hedge_shares: -30,
            net_delta: 20.0,
            gross_delta_pnl: 100.0,
            hedge_delta_pnl: -60.0,
            net_delta_pnl: 40.0,
            gamma_pnl: 10.0,
            theta_pnl: -20.0,
            vega_pnl: 60.0,
        };

        let day2 = DailyAttribution {
            date: chrono::NaiveDate::from_ymd_opt(2025, 1, 2).unwrap(),
            spot_open: 102.0,
            spot_close: 101.0,
            spot_change: -1.0,
            iv_open: 0.32,
            iv_close: 0.31,
            iv_change: -0.01,
            option_delta: 60.0,
            option_gamma: 5.5,
            hedge_shares: -30,
            net_delta: 30.0,
            gross_delta_pnl: -60.0,
            hedge_delta_pnl: 30.0,
            net_delta_pnl: -30.0,
            gamma_pnl: 2.75,
            theta_pnl: -19.0,
            vega_pnl: -30.0,
        };

        let actual_pnl = Decimal::new(63, 0);  // $63
        let attr = PositionAttribution::from_daily(vec![day1, day2], actual_pnl);

        // Total gross delta = 100 + (-60) = 40
        assert_eq!(attr.total_gross_delta_pnl, Decimal::new(40, 0));

        // Total hedge delta = -60 + 30 = -30
        assert_eq!(attr.total_hedge_delta_pnl, Decimal::new(-30, 0));

        // Total net delta = 40 + (-30) = 10
        assert_eq!(attr.total_net_delta_pnl, Decimal::new(10, 0));

        // Total gamma = 10 + 2.75 = 12.75
        assert_eq!(attr.total_gamma_pnl, Decimal::try_from(12.75).unwrap());

        // Total theta = -20 + (-19) = -39
        assert_eq!(attr.total_theta_pnl, Decimal::new(-39, 0));

        // Total vega = 60 + (-30) = 30
        assert_eq!(attr.total_vega_pnl, Decimal::new(30, 0));

        // Explained = 10 + 12.75 - 39 + 30 = 13.75
        // Unexplained = 63 - 13.75 = 49.25
        assert_eq!(attr.total_unexplained, Decimal::try_from(49.25).unwrap());

        // Hedge efficiency = |-30| / |40| × 100 = 75%
        assert!((attr.hedge_efficiency - 75.0).abs() < 0.01);

        assert_eq!(attr.num_days(), 2);
    }

    #[test]
    fn test_hedge_efficiency_perfect() {
        // Perfect hedge: hedge completely offsets gross delta
        let day = DailyAttribution {
            date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            spot_open: 100.0,
            spot_close: 102.0,
            spot_change: 2.0,
            iv_open: 0.30,
            iv_close: 0.30,
            iv_change: 0.0,
            option_delta: 50.0,
            option_gamma: 5.0,
            hedge_shares: -50,  // Perfectly hedged
            net_delta: 0.0,
            gross_delta_pnl: 100.0,
            hedge_delta_pnl: -100.0,  // Fully offsets
            net_delta_pnl: 0.0,
            gamma_pnl: 10.0,
            theta_pnl: -20.0,
            vega_pnl: 0.0,
        };

        let attr = PositionAttribution::from_daily(vec![day], Decimal::ZERO);

        // Hedge efficiency = |-100| / |100| × 100 = 100%
        assert!((attr.hedge_efficiency - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_hedge_efficiency_no_hedge() {
        // No hedge: hedge_delta_pnl = 0
        let day = DailyAttribution {
            date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            spot_open: 100.0,
            spot_close: 102.0,
            spot_change: 2.0,
            iv_open: 0.30,
            iv_close: 0.30,
            iv_change: 0.0,
            option_delta: 50.0,
            option_gamma: 5.0,
            hedge_shares: 0,  // No hedge
            net_delta: 50.0,
            gross_delta_pnl: 100.0,
            hedge_delta_pnl: 0.0,
            net_delta_pnl: 100.0,
            gamma_pnl: 10.0,
            theta_pnl: -20.0,
            vega_pnl: 0.0,
        };

        let attr = PositionAttribution::from_daily(vec![day], Decimal::ZERO);

        // Hedge efficiency = 0 / 100 × 100 = 0%
        assert_eq!(attr.hedge_efficiency, 0.0);
    }
}
