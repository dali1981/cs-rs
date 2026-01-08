//! Helper functions for computing Greeks across options strategies

use cs_analytics::Greeks;

/// Compute net Greeks for a straddle (long call + long put)
/// Both positions are long, so greeks are summed
pub fn compute_straddle_greeks(
    call_greeks: Option<Greeks>,
    put_greeks: Option<Greeks>,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (call_greeks, put_greeks) {
        (Some(call_g), Some(put_g)) => {
            // Long both legs: add greeks
            let net_delta = call_g.delta + put_g.delta;
            let net_gamma = call_g.gamma + put_g.gamma;
            let net_theta = call_g.theta + put_g.theta;
            let net_vega = call_g.vega + put_g.vega;

            (
                Some(net_delta),
                Some(net_gamma),
                Some(net_theta),
                Some(net_vega),
            )
        }
        _ => (None, None, None, None),
    }
}

/// Compute net Greeks for a spread (short leg + long leg)
/// Short leg has negative sign, long leg has positive sign
pub fn compute_spread_net_greeks(
    short_greeks: Option<Greeks>,
    long_greeks: Option<Greeks>,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (short_greeks, long_greeks) {
        (Some(short_g), Some(long_g)) => {
            // Short leg: negative sign, long leg: positive sign
            let net_delta = long_g.delta - short_g.delta;
            let net_gamma = long_g.gamma - short_g.gamma;
            let net_theta = long_g.theta - short_g.theta;
            let net_vega = long_g.vega - short_g.vega;

            (
                Some(net_delta),
                Some(net_gamma),
                Some(net_theta),
                Some(net_vega),
            )
        }
        _ => (None, None, None, None),
    }
}

/// Compute net Greeks for iron butterfly (4 legs with 2 short, 2 long)
/// Short call + short put - long call - long put
pub fn compute_iron_butterfly_net_greeks(
    short_call_greeks: Option<Greeks>,
    short_put_greeks: Option<Greeks>,
    long_call_greeks: Option<Greeks>,
    long_put_greeks: Option<Greeks>,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (short_call_greeks, short_put_greeks, long_call_greeks, long_put_greeks) {
        (Some(sc), Some(sp), Some(lc), Some(lp)) => {
            // Short legs contribute negatively, long legs contribute positively
            let net_delta = sc.delta + sp.delta - lc.delta - lp.delta;
            let net_gamma = sc.gamma + sp.gamma - lc.gamma - lp.gamma;
            let net_theta = sc.theta + sp.theta - lc.theta - lp.theta;
            let net_vega = sc.vega + sp.vega - lc.vega - lp.vega;

            (Some(net_delta), Some(net_gamma), Some(net_theta), Some(net_vega))
        }
        _ => (None, None, None, None),
    }
}

/// Compute net Greeks for calendar straddle (2 short + 2 long)
/// Short near-term legs - long far-term legs
pub fn compute_calendar_straddle_net_greeks(
    short_call_greeks: Option<Greeks>,
    short_put_greeks: Option<Greeks>,
    long_call_greeks: Option<Greeks>,
    long_put_greeks: Option<Greeks>,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    match (short_call_greeks, short_put_greeks, long_call_greeks, long_put_greeks) {
        (Some(sc), Some(sp), Some(lc), Some(lp)) => {
            // Short near-term legs - long far-term legs
            let net_delta = sc.delta + sp.delta - lc.delta - lp.delta;
            let net_gamma = sc.gamma + sp.gamma - lc.gamma - lp.gamma;
            let net_theta = sc.theta + sp.theta - lc.theta - lp.theta;
            let net_vega = sc.vega + sp.vega - lc.vega - lp.vega;

            (Some(net_delta), Some(net_gamma), Some(net_theta), Some(net_vega))
        }
        _ => (None, None, None, None),
    }
}

/// Compute IV change between two time points for two-leg instruments
/// Handles cases where one or both IV values may be missing
pub fn compute_iv_change(entry_iv: Option<f64>, exit_iv: Option<f64>) -> (Option<f64>, Option<f64>, Option<f64>) {
    let iv_change = match (entry_iv, exit_iv) {
        (Some(entry), Some(exit)) => Some(exit - entry),
        _ => None,
    };

    (entry_iv, exit_iv, iv_change)
}

/// Average IV values from multiple legs with fallback logic
/// Returns average if both present, else returns whichever is available
pub fn average_iv(iv1: Option<f64>, iv2: Option<f64>) -> Option<f64> {
    match (iv1, iv2) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_straddle_greeks_valid() {
        let call = Greeks {
            delta: 0.5,
            gamma: 0.1,
            theta: -0.01,
            vega: 0.3,
        };
        let put = Greeks {
            delta: -0.5,
            gamma: 0.1,
            theta: -0.01,
            vega: 0.3,
        };

        let (delta, gamma, theta, vega) = compute_straddle_greeks(Some(call), Some(put));
        assert_eq!(delta, Some(0.0));
        assert_eq!(gamma, Some(0.2));
        assert_eq!(theta, Some(-0.02));
        assert_eq!(vega, Some(0.6));
    }

    #[test]
    fn test_compute_straddle_greeks_missing() {
        let (delta, gamma, theta, vega) = compute_straddle_greeks(None, None);
        assert_eq!(delta, None);
        assert_eq!(gamma, None);
        assert_eq!(theta, None);
        assert_eq!(vega, None);
    }

    #[test]
    fn test_compute_spread_net_greeks() {
        let short = Greeks {
            delta: 0.3,
            gamma: 0.05,
            theta: -0.005,
            vega: 0.1,
        };
        let long = Greeks {
            delta: 0.7,
            gamma: 0.15,
            theta: -0.015,
            vega: 0.3,
        };

        let (delta, gamma, theta, vega) = compute_spread_net_greeks(Some(short), Some(long));
        assert_eq!(delta, Some(0.4));
        assert_eq!(gamma, Some(0.1));
        assert_eq!(theta, Some(-0.01));
        assert_eq!(vega, Some(0.2));
    }

    #[test]
    fn test_compute_iv_change() {
        let (entry, exit, change) = compute_iv_change(Some(0.25), Some(0.30));
        assert_eq!(entry, Some(0.25));
        assert_eq!(exit, Some(0.30));
        assert_eq!(change, Some(0.05));
    }

    #[test]
    fn test_average_iv() {
        assert_eq!(average_iv(Some(0.25), Some(0.30)), Some(0.275));
        assert_eq!(average_iv(Some(0.25), None), Some(0.25));
        assert_eq!(average_iv(None, Some(0.30)), Some(0.30));
        assert_eq!(average_iv(None, None), None);
    }
}
