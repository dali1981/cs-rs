use std::ops::{Add, Sub, Mul, Neg};

/// Option Greeks - immutable value object
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Greeks {
    pub delta: f64,
    pub gamma: f64,
    pub theta: f64,  // Per day
    pub vega: f64,   // Per 1% vol change
    pub rho: f64,    // Per 1% rate change
}

impl Greeks {
    pub const ZERO: Greeks = Greeks {
        delta: 0.0,
        gamma: 0.0,
        theta: 0.0,
        vega: 0.0,
        rho: 0.0,
    };

    /// Greeks at expiry (delta only)
    pub fn at_expiry(spot: f64, strike: f64, is_call: bool) -> Self {
        let delta = if is_call {
            if spot > strike { 1.0 } else { 0.0 }
        } else {
            if spot < strike { -1.0 } else { 0.0 }
        };
        Self { delta, ..Self::ZERO }
    }

    /// Spread Greeks = long - short
    pub fn spread(long: &Greeks, short: &Greeks) -> Greeks {
        *long - *short
    }

    /// Position Greeks = greeks * signed_quantity
    pub fn position(&self, quantity: i32) -> Greeks {
        *self * (quantity as f64)
    }
}

impl Add for Greeks {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            delta: self.delta + other.delta,
            gamma: self.gamma + other.gamma,
            theta: self.theta + other.theta,
            vega: self.vega + other.vega,
            rho: self.rho + other.rho,
        }
    }
}

impl Sub for Greeks {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self {
            delta: self.delta - other.delta,
            gamma: self.gamma - other.gamma,
            theta: self.theta - other.theta,
            vega: self.vega - other.vega,
            rho: self.rho - other.rho,
        }
    }
}

impl Mul<f64> for Greeks {
    type Output = Self;
    fn mul(self, scalar: f64) -> Self {
        Self {
            delta: self.delta * scalar,
            gamma: self.gamma * scalar,
            theta: self.theta * scalar,
            vega: self.vega * scalar,
            rho: self.rho * scalar,
        }
    }
}

impl Neg for Greeks {
    type Output = Self;
    fn neg(self) -> Self {
        self * -1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greeks_zero() {
        let g = Greeks::ZERO;
        assert_eq!(g.delta, 0.0);
        assert_eq!(g.gamma, 0.0);
        assert_eq!(g.theta, 0.0);
        assert_eq!(g.vega, 0.0);
        assert_eq!(g.rho, 0.0);
    }

    #[test]
    fn test_greeks_at_expiry_call() {
        let g = Greeks::at_expiry(105.0, 100.0, true);
        assert_eq!(g.delta, 1.0);
        assert_eq!(g.gamma, 0.0);

        let g_otm = Greeks::at_expiry(95.0, 100.0, true);
        assert_eq!(g_otm.delta, 0.0);
    }

    #[test]
    fn test_greeks_at_expiry_put() {
        let g = Greeks::at_expiry(95.0, 100.0, false);
        assert_eq!(g.delta, -1.0);

        let g_otm = Greeks::at_expiry(105.0, 100.0, false);
        assert_eq!(g_otm.delta, 0.0);
    }

    #[test]
    fn test_greeks_addition() {
        let g1 = Greeks { delta: 0.5, gamma: 0.1, theta: -0.05, vega: 0.2, rho: 0.01 };
        let g2 = Greeks { delta: 0.3, gamma: 0.05, theta: -0.02, vega: 0.1, rho: 0.005 };
        let sum = g1 + g2;

        assert!((sum.delta - 0.8).abs() < 1e-10);
        assert!((sum.gamma - 0.15).abs() < 1e-10);
        assert!((sum.theta + 0.07).abs() < 1e-10);
        assert!((sum.vega - 0.3).abs() < 1e-10);
        assert!((sum.rho - 0.015).abs() < 1e-10);
    }

    #[test]
    fn test_greeks_subtraction() {
        let long = Greeks { delta: 0.6, gamma: 0.1, theta: -0.05, vega: 0.2, rho: 0.01 };
        let short = Greeks { delta: 0.4, gamma: 0.05, theta: -0.02, vega: 0.1, rho: 0.005 };
        let spread = long - short;

        assert!((spread.delta - 0.2).abs() < 1e-10);
        assert!((spread.gamma - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_greeks_spread_helper() {
        let long = Greeks { delta: 0.6, gamma: 0.1, theta: -0.05, vega: 0.2, rho: 0.01 };
        let short = Greeks { delta: 0.4, gamma: 0.05, theta: -0.02, vega: 0.1, rho: 0.005 };
        let spread = Greeks::spread(&long, &short);

        assert!((spread.delta - 0.2).abs() < 1e-10);
    }

    #[test]
    fn test_greeks_scalar_multiplication() {
        let g = Greeks { delta: 0.5, gamma: 0.1, theta: -0.05, vega: 0.2, rho: 0.01 };
        let scaled = g * 2.0;

        assert!((scaled.delta - 1.0).abs() < 1e-10);
        assert!((scaled.gamma - 0.2).abs() < 1e-10);
        assert!((scaled.theta + 0.1).abs() < 1e-10);
    }

    #[test]
    fn test_greeks_position() {
        let g = Greeks { delta: 0.5, gamma: 0.1, theta: -0.05, vega: 0.2, rho: 0.01 };
        let position = g.position(10);

        assert!((position.delta - 5.0).abs() < 1e-10);
        assert!((position.gamma - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_greeks_negation() {
        let g = Greeks { delta: 0.5, gamma: 0.1, theta: -0.05, vega: 0.2, rho: 0.01 };
        let neg = -g;

        assert!((neg.delta + 0.5).abs() < 1e-10);
        assert!((neg.gamma + 0.1).abs() < 1e-10);
        assert!((neg.theta - 0.05).abs() < 1e-10);
    }
}
