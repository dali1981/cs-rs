use rust_decimal::Decimal;
use cs_analytics::Greeks;

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

/// Calculate P&L attribution from Greeks
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
}
