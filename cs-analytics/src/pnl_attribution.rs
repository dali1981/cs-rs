use rust_decimal::Decimal;
use crate::Greeks;

/// P&L attribution breakdown
#[derive(Debug, Clone, Default)]
pub struct PnLAttribution {
    pub total: Decimal,
    pub delta: Decimal,
    pub gamma: Decimal,
    pub theta: Decimal,
    pub vega: Decimal,
    pub unexplained: Decimal,
}

/// Leg-level P&L components (returns f64 for easy summing)
#[derive(Debug, Clone, Default)]
pub struct LegPnL {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,
    pub vega: f64,
}

/// Calculate P&L attribution from Greeks (single leg or already-netted spread greeks)
pub fn calculate_pnl_attribution(
    entry_greeks: &Greeks,
    spot_change: f64,
    iv_change: f64,
    days_held: f64,
    total_pnl: Decimal,
) -> PnLAttribution {
    let delta_pnl = Decimal::try_from(entry_greeks.delta * spot_change).unwrap_or_default();
    let gamma_pnl = Decimal::try_from(0.5 * entry_greeks.gamma * spot_change.powi(2)).unwrap_or_default();
    let theta_pnl = Decimal::try_from(entry_greeks.theta * days_held).unwrap_or_default();
    let vega_pnl = Decimal::try_from(entry_greeks.vega * iv_change * 100.0).unwrap_or_default();

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl - explained;

    PnLAttribution {
        total: total_pnl,
        delta: delta_pnl,
        gamma: gamma_pnl,
        theta: theta_pnl,
        vega: vega_pnl,
        unexplained,
    }
}

/// Calculate P&L attribution for a spread with separate legs
///
/// This is the CORRECT calculation for multi-leg strategies where each leg
/// has independent IV changes. For a calendar spread:
/// - We are SHORT the short leg (sell near-term)
/// - We are LONG the long leg (buy far-term)
/// - Each leg has different vega and different IV change
///
/// Vega P&L must be calculated per-leg:
/// - Short leg: -short_vega * short_iv_change * 100 (negative because we're short)
/// - Long leg:  +long_vega * long_iv_change * 100 (positive because we're long)
pub fn calculate_spread_pnl_attribution(
    short_greeks: &Greeks,
    long_greeks: &Greeks,
    spot_change: f64,
    short_iv_change: f64,
    long_iv_change: f64,
    days_held: f64,
    total_pnl: Decimal,
) -> PnLAttribution {
    // Net greeks (long - short)
    let net_delta = long_greeks.delta - short_greeks.delta;
    let net_gamma = long_greeks.gamma - short_greeks.gamma;
    let net_theta = long_greeks.theta - short_greeks.theta;

    // First-order greeks
    let delta_pnl = Decimal::try_from(net_delta * spot_change).unwrap_or_default();
    let gamma_pnl = Decimal::try_from(0.5 * net_gamma * spot_change.powi(2)).unwrap_or_default();
    let theta_pnl = Decimal::try_from(net_theta * days_held).unwrap_or_default();

    // Vega P&L calculated per-leg with correct signs
    // Short leg: we are SHORT, so IV increase hurts us (negative vega exposure)
    // Long leg: we are LONG, so IV increase helps us (positive vega exposure)
    let short_vega_pnl = Decimal::try_from(-short_greeks.vega * short_iv_change * 100.0).unwrap_or_default();
    let long_vega_pnl = Decimal::try_from(long_greeks.vega * long_iv_change * 100.0).unwrap_or_default();
    let vega_pnl = short_vega_pnl + long_vega_pnl;

    let explained = delta_pnl + gamma_pnl + theta_pnl + vega_pnl;
    let unexplained = total_pnl - explained;

    PnLAttribution {
        total: total_pnl,
        delta: delta_pnl,
        gamma: gamma_pnl,
        theta: theta_pnl,
        vega: vega_pnl,
        unexplained,
    }
}

/// Calculate P&L attribution for a single option leg
///
/// This is the fundamental building block for all multi-leg strategies.
/// Each leg's P&L is calculated independently and then summed.
///
/// # Arguments
/// * `entry_greeks` - Greeks at entry time
/// * `entry_iv` - Implied volatility at entry (as decimal, e.g., 0.30 = 30%)
/// * `exit_iv` - Implied volatility at exit (as decimal, e.g., 0.35 = 35%)
/// * `spot_change` - Change in underlying price (exit_spot - entry_spot)
/// * `days_held` - Number of days position was held
/// * `position_sign` - +1.0 for long positions, -1.0 for short positions
///
/// # Returns
/// LegPnL with delta, gamma, theta, vega components in f64 (for easy summing)
///
/// # Formula
/// - Delta P&L = position_sign × delta × spot_change
/// - Gamma P&L = position_sign × 0.5 × gamma × spot_change²
/// - Theta P&L = position_sign × theta × days_held
/// - Vega P&L = position_sign × vega × (exit_iv - entry_iv) × 100
///
/// The factor of 100 in vega is because:
/// - Vega is quoted as P&L per 1% IV change
/// - IV is stored as decimal (0.30 not 30)
/// - So IV change of 0.05 = 5% needs × 100
pub fn calculate_option_leg_pnl(
    entry_greeks: Option<&Greeks>,
    entry_iv: Option<f64>,
    exit_iv: Option<f64>,
    spot_change: f64,
    days_held: f64,
    position_sign: f64,
) -> LegPnL {
    match entry_greeks {
        Some(greeks) => {
            let delta_pnl = position_sign * greeks.delta * spot_change;
            let gamma_pnl = position_sign * 0.5 * greeks.gamma * spot_change.powi(2);
            let theta_pnl = position_sign * greeks.theta * days_held;

            let vega_pnl = match (entry_iv, exit_iv) {
                (Some(iv_entry), Some(iv_exit)) => {
                    let iv_change = iv_exit - iv_entry;
                    position_sign * greeks.vega * iv_change * 100.0
                }
                _ => 0.0,
            };

            LegPnL {
                delta: delta_pnl,
                gamma: gamma_pnl,
                theta: theta_pnl,
                vega: vega_pnl,
            }
        }
        None => LegPnL::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pnl_attribution_delta() {
        let greeks = Greeks {
            delta: 0.5,
            gamma: 0.0,
            theta: 0.0,
            vega: 0.0,
            rho: 0.0,
        };

        let attr = calculate_pnl_attribution(
            &greeks,
            2.0,  // $2 spot increase
            0.0,  // No IV change
            0.0,  // No time passed
            Decimal::new(1, 0),  // $1 total P&L
        );

        assert_eq!(attr.delta, Decimal::new(1, 0)); // 0.5 * 2.0 = 1.0
        assert_eq!(attr.gamma, Decimal::ZERO);
        assert_eq!(attr.theta, Decimal::ZERO);
        assert_eq!(attr.vega, Decimal::ZERO);
        assert_eq!(attr.unexplained, Decimal::ZERO);
    }

    #[test]
    fn test_pnl_attribution_gamma() {
        let greeks = Greeks {
            delta: 0.0,
            gamma: 0.1,
            theta: 0.0,
            vega: 0.0,
            rho: 0.0,
        };

        let attr = calculate_pnl_attribution(
            &greeks,
            10.0,  // $10 spot change
            0.0,
            0.0,
            Decimal::new(5, 0),
        );

        // 0.5 * 0.1 * 10^2 = 5.0
        assert_eq!(attr.gamma, Decimal::new(5, 0));
    }

    #[test]
    fn test_pnl_attribution_theta() {
        let greeks = Greeks {
            delta: 0.0,
            gamma: 0.0,
            theta: -0.05,  // -$0.05 per day
            vega: 0.0,
            rho: 0.0,
        };

        let attr = calculate_pnl_attribution(
            &greeks,
            0.0,
            0.0,
            1.0,  // 1 day
            Decimal::new(-5, 2),  // -$0.05
        );

        assert_eq!(attr.theta, Decimal::new(-5, 2));
        assert_eq!(attr.unexplained, Decimal::ZERO);
    }

    #[test]
    fn test_pnl_attribution_vega() {
        let greeks = Greeks {
            delta: 0.0,
            gamma: 0.0,
            theta: 0.0,
            vega: 0.2,  // $0.20 per 1% IV change
            rho: 0.0,
        };

        let attr = calculate_pnl_attribution(
            &greeks,
            0.0,
            0.05,  // 5% IV increase (0.30 -> 0.35)
            0.0,
            Decimal::new(1, 0),
        );

        // 0.2 * 0.05 * 100 = 1.0
        assert_eq!(attr.vega, Decimal::new(1, 0));
    }

    #[test]
    fn test_pnl_attribution_combined() {
        let greeks = Greeks {
            delta: 0.5,
            gamma: 0.05,
            theta: -0.1,
            vega: 0.3,
            rho: 0.0,
        };

        let attr = calculate_pnl_attribution(
            &greeks,
            5.0,   // $5 spot increase
            0.02,  // 2% IV increase
            1.0,   // 1 day
            Decimal::new(35, 1),  // $3.50
        );

        // Delta: 0.5 * 5 = 2.5
        // Gamma: 0.5 * 0.05 * 25 = 0.625
        // Theta: -0.1 * 1 = -0.1
        // Vega: 0.3 * 0.02 * 100 = 0.6
        // Total explained: 2.5 + 0.625 - 0.1 + 0.6 = 3.625
        // Unexplained: 3.5 - 3.625 = -0.125

        assert!(attr.delta > Decimal::ZERO);
        assert!(attr.gamma > Decimal::ZERO);
        assert!(attr.theta < Decimal::ZERO);
        assert!(attr.vega > Decimal::ZERO);
    }

    #[test]
    fn test_pnl_attribution_unexplained() {
        let greeks = Greeks::ZERO;

        let attr = calculate_pnl_attribution(
            &greeks,
            0.0,
            0.0,
            0.0,
            Decimal::new(100, 0),  // $100 P&L but no Greeks explanation
        );

        assert_eq!(attr.unexplained, Decimal::new(100, 0));
    }

    #[test]
    fn test_calendar_spread_vega_bug() {
        // This test demonstrates the bug in the original implementation
        // Based on NTAP trade: short IV increased, long IV decreased
        let short_greeks = Greeks {
            delta: 0.5019,
            gamma: 0.0632,
            theta: -0.4197,
            vega: 0.0426,
            rho: 0.0,
        };

        let long_greeks = Greeks {
            delta: 0.5174,
            gamma: 0.0522,
            theta: -0.0971,
            vega: 0.1006,
            rho: 0.0,
        };

        let spot_change = 108.7276 - 116.81;  // -8.08
        let short_iv_change = 0.8103 - 0.5906;  // +0.2197 (IV exploded)
        let long_iv_change = 0.2797 - 0.3025;  // -0.0228 (IV decreased)
        let days_held = 1.433;
        let actual_pnl = Decimal::try_from(-0.586357098815967).unwrap();

        // OLD (BUGGY) METHOD: Using only short IV change
        let spread_greeks = long_greeks - short_greeks;
        let buggy_attr = calculate_pnl_attribution(
            &spread_greeks,
            spot_change,
            short_iv_change,  // Only using short leg IV change!
            days_held,
            actual_pnl,
        );

        // CORRECT METHOD: Using per-leg IV changes
        let correct_attr = calculate_spread_pnl_attribution(
            &short_greeks,
            &long_greeks,
            spot_change,
            short_iv_change,
            long_iv_change,
            days_held,
            actual_pnl,
        );

        // Delta and gamma should be the same
        assert_eq!(buggy_attr.delta, correct_attr.delta);
        assert_eq!(buggy_attr.gamma, correct_attr.gamma);
        assert_eq!(buggy_attr.theta, correct_attr.theta);

        // Vega should be VERY different
        // Buggy: +1.27 (positive, thinks we benefited from IV increase)
        // Correct: -1.17 (negative, we lost money from IV changes)
        assert!(buggy_attr.vega > Decimal::ZERO);  // Buggy shows positive
        assert!(correct_attr.vega < Decimal::ZERO);  // Correct shows negative

        // The difference should be about $2.44
        let vega_diff = buggy_attr.vega - correct_attr.vega;
        assert!(vega_diff > Decimal::try_from(2.0).unwrap());
        assert!(vega_diff < Decimal::try_from(3.0).unwrap());

        // Unexplained should be much smaller with correct method
        assert!(correct_attr.unexplained.abs() < buggy_attr.unexplained.abs());
    }

    #[test]
    fn test_calendar_spread_vega_both_increase() {
        // Test case: Both IVs increase, but by different amounts
        let short_greeks = Greeks {
            delta: 0.5,
            gamma: 0.05,
            theta: -0.3,
            vega: 0.1,  // Short vega
            rho: 0.0,
        };

        let long_greeks = Greeks {
            delta: 0.52,
            gamma: 0.04,
            theta: -0.05,
            vega: 0.25,  // Long vega (higher)
            rho: 0.0,
        };

        let spot_change = 0.0;  // No spot move
        let short_iv_change = 0.30;  // Short IV up 30 pts
        let long_iv_change = 0.10;   // Long IV up 10 pts
        let days_held = 1.0;

        // Expected vega P&L:
        // Short: -0.1 * 0.30 * 100 = -3.0 (we're short, IV up hurts us)
        // Long:  +0.25 * 0.10 * 100 = +2.5 (we're long, IV up helps us)
        // Net vega:   -3.0 + 2.5 = -0.5
        // Net theta: (-0.05 - (-0.3)) * 1.0 = 0.25
        // Total: -0.5 + 0.25 = -0.25
        let expected_pnl = Decimal::try_from(-0.25).unwrap();

        let attr = calculate_spread_pnl_attribution(
            &short_greeks,
            &long_greeks,
            spot_change,
            short_iv_change,
            long_iv_change,
            days_held,
            expected_pnl,
        );

        // Vega should be close to -0.5
        let expected_vega = Decimal::try_from(-0.5).unwrap();
        let tolerance = Decimal::try_from(0.01).unwrap();
        assert!((attr.vega - expected_vega).abs() < tolerance);

        // Theta should be close to 0.25
        let expected_theta = Decimal::try_from(0.25).unwrap();
        assert!((attr.theta - expected_theta).abs() < tolerance);

        assert!(attr.unexplained.abs() < tolerance);
    }

    #[test]
    fn test_calendar_spread_vega_both_decrease() {
        // Test case: IV crush on both legs (typical post-earnings)
        let short_greeks = Greeks {
            delta: 0.5,
            gamma: 0.05,
            theta: -0.3,
            vega: 0.1,
            rho: 0.0,
        };

        let long_greeks = Greeks {
            delta: 0.52,
            gamma: 0.04,
            theta: -0.05,
            vega: 0.25,
            rho: 0.0,
        };

        let spot_change = 0.0;
        let short_iv_change = -0.50;  // Short IV down 50 pts (big crush)
        let long_iv_change = -0.20;   // Long IV down 20 pts (smaller crush)
        let days_held = 1.0;

        // Expected vega P&L:
        // Short: -0.1 * (-0.50) * 100 = +5.0 (we're short, IV down helps us)
        // Long:  +0.25 * (-0.20) * 100 = -5.0 (we're long, IV down hurts us)
        // Net vega:   +5.0 - 5.0 = 0.0
        // Net theta: (-0.05 - (-0.3)) * 1.0 = 0.25
        // Total: 0.0 + 0.25 = 0.25
        let expected_pnl = Decimal::try_from(0.25).unwrap();

        let attr = calculate_spread_pnl_attribution(
            &short_greeks,
            &long_greeks,
            spot_change,
            short_iv_change,
            long_iv_change,
            days_held,
            expected_pnl,
        );

        // Vega should be close to 0 (both effects cancel out)
        let tolerance = Decimal::try_from(0.01).unwrap();
        assert!(attr.vega.abs() < tolerance);

        // Theta should be close to 0.25
        let expected_theta = Decimal::try_from(0.25).unwrap();
        assert!((attr.theta - expected_theta).abs() < tolerance);

        assert!(attr.unexplained.abs() < tolerance);
    }
}
