use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use cs_analytics::Greeks;

/// Position-level Greeks (already scaled by contract multiplier)
/// These represent real P&L exposure, not per-share values
///
/// Example for long ATM straddle with 100 multiplier:
/// - delta: +50 (call) - 50 (put) ≈ 0 (not 0.0)
/// - gamma: +5.0 (combined long gamma)
/// - theta: -20 per day (time decay)
/// - vega: +30 per 1% IV move
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PositionGreeks {
    pub delta: f64,  // Position-level: e.g., +50 for 0.5 delta × 100 shares
    pub gamma: f64,  // Position-level: e.g., +5 for 0.05 gamma × 100 shares
    pub theta: f64,  // Position-level: e.g., -20 for -0.20 theta × 100 shares
    pub vega: f64,   // Position-level: e.g., +30 for 0.30 vega × 100 shares
}

impl PositionGreeks {
    /// Convert from per-share Greeks to position-level Greeks
    ///
    /// # Arguments
    /// * `greeks` - Per-share Greeks from pricing model
    /// * `multiplier` - Contract multiplier (typically 100 for equity options)
    ///
    /// # Example
    /// ```
    /// let per_share = Greeks { delta: 0.5, gamma: 0.03, theta: -0.15, vega: 0.25, rho: 0.0 };
    /// let position = PositionGreeks::from_per_share(&per_share, 100);
    /// assert_eq!(position.delta, 50.0);
    /// assert_eq!(position.gamma, 3.0);
    /// ```
    pub fn from_per_share(greeks: &Greeks, multiplier: i32) -> Self {
        let m = multiplier as f64;
        Self {
            delta: greeks.delta * m,
            gamma: greeks.gamma * m,
            theta: greeks.theta * m,
            vega: greeks.vega * m,
        }
    }

    /// Combine call + put Greeks for a straddle position
    ///
    /// # Arguments
    /// * `call` - Per-share Greeks for the call leg
    /// * `put` - Per-share Greeks for the put leg
    /// * `multiplier` - Contract multiplier (typically 100)
    ///
    /// # Example
    /// Long ATM straddle:
    /// - Call: delta=0.5, gamma=0.03, theta=-0.10, vega=0.15
    /// - Put: delta=-0.5, gamma=0.03, theta=-0.10, vega=0.15
    /// - Combined (×100): delta≈0, gamma=6.0, theta=-20, vega=30
    pub fn straddle(call: &Greeks, put: &Greeks, multiplier: i32) -> Self {
        let combined = *call + *put;
        Self::from_per_share(&combined, multiplier)
    }
}

/// A snapshot of position state at a point in time
/// Greeks are recomputed daily from the IV surface (not carried forward from entry)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSnapshot {
    pub timestamp: DateTime<Utc>,
    pub spot: f64,
    pub iv: f64,                       // IV at snapshot time (for vega attribution)
    pub option_greeks: PositionGreeks, // Recomputed from current spot/IV/DTE
    pub hedge_shares: i32,             // Negative = short
    pub net_delta: f64,                // option_delta + hedge_shares
}

impl PositionSnapshot {
    /// Create snapshot with freshly computed Greeks
    /// Greeks should be recomputed from the IV surface at this timestamp
    ///
    /// # Arguments
    /// * `timestamp` - Time of snapshot
    /// * `spot` - Spot price at this time
    /// * `iv` - Implied volatility at this time
    /// * `option_greeks` - Position-level Greeks (already scaled by multiplier)
    /// * `hedge_shares` - Current hedge position (negative for short)
    pub fn new(
        timestamp: DateTime<Utc>,
        spot: f64,
        iv: f64,
        option_greeks: PositionGreeks,
        hedge_shares: i32,
    ) -> Self {
        let net_delta = option_greeks.delta + hedge_shares as f64;
        Self {
            timestamp,
            spot,
            iv,
            option_greeks,
            hedge_shares,
            net_delta,
        }
    }

    /// Intraday delta approximation using gamma (between full recomputations)
    /// Only used for hedge trigger checks, NOT for P&L attribution
    ///
    /// This is a cheap approximation for checking if rehedge is needed.
    /// For actual P&L attribution, Greeks must be recomputed from IV surface.
    pub fn with_gamma_adjusted_delta(&self, new_spot: f64) -> Self {
        let spot_change = new_spot - self.spot;
        let new_option_delta = self.option_greeks.delta
            + self.option_greeks.gamma * spot_change;

        Self {
            option_greeks: PositionGreeks {
                delta: new_option_delta,
                ..self.option_greeks
            },
            net_delta: new_option_delta + self.hedge_shares as f64,
            spot: new_spot,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_greeks_from_per_share() {
        let per_share = Greeks {
            delta: 0.5,
            gamma: 0.03,
            theta: -0.15,
            vega: 0.25,
            rho: 0.0,
        };

        let position = PositionGreeks::from_per_share(&per_share, 100);

        assert_eq!(position.delta, 50.0);
        assert_eq!(position.gamma, 3.0);
        assert_eq!(position.theta, -15.0);
        assert_eq!(position.vega, 25.0);
    }

    #[test]
    fn test_position_greeks_straddle() {
        let call = Greeks {
            delta: 0.5,
            gamma: 0.03,
            theta: -0.10,
            vega: 0.15,
            rho: 0.0,
        };

        let put = Greeks {
            delta: -0.5,
            gamma: 0.03,
            theta: -0.10,
            vega: 0.15,
            rho: 0.0,
        };

        let straddle = PositionGreeks::straddle(&call, &put, 100);

        // Delta should be near zero for ATM straddle
        assert!((straddle.delta - 0.0).abs() < 0.01);
        // Gamma adds (both legs positive)
        assert_eq!(straddle.gamma, 6.0);
        // Theta adds (both legs negative)
        assert_eq!(straddle.theta, -20.0);
        // Vega adds (both legs positive)
        assert_eq!(straddle.vega, 30.0);
    }

    #[test]
    fn test_position_snapshot_net_delta() {
        let greeks = PositionGreeks {
            delta: 50.0,
            gamma: 5.0,
            theta: -20.0,
            vega: 30.0,
        };

        let snapshot = PositionSnapshot::new(
            Utc::now(),
            100.0,
            0.30,
            greeks,
            -30, // Short 30 shares
        );

        // Net delta = 50 (options) - 30 (hedge) = 20
        assert_eq!(snapshot.net_delta, 20.0);
    }

    #[test]
    fn test_position_snapshot_gamma_adjustment() {
        let greeks = PositionGreeks {
            delta: 50.0,
            gamma: 5.0,
            theta: -20.0,
            vega: 30.0,
        };

        let snapshot = PositionSnapshot::new(
            Utc::now(),
            100.0,
            0.30,
            greeks,
            0,
        );

        // Spot moves from 100 to 102 (+2)
        let adjusted = snapshot.with_gamma_adjusted_delta(102.0);

        // New delta ≈ 50 + (5 × 2) = 60
        assert!((adjusted.option_greeks.delta - 60.0).abs() < 0.01);
        assert_eq!(adjusted.spot, 102.0);
    }
}
