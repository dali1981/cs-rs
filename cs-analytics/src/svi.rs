// SVI (Stochastic Volatility Inspired) parameterization
//
// The industry standard for equity volatility surfaces.
// Reference: Gatheral & Jacquier (2014) "Arbitrage-free SVI volatility surfaces"

use serde::{Deserialize, Serialize};

/// SVI parameterization for a single expiry slice
///
/// Total variance: w(k) = a + b * (ρ(k - m) + √((k - m)² + σ²))
///
/// Where:
/// - k = log-moneyness = ln(K/F)
/// - w = total variance = σ² × τ
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SVIParams {
    /// Minimum variance level (vertical shift)
    pub a: f64,
    /// Slope of the wings (controls overall vol level)
    pub b: f64,
    /// Correlation/skew parameter (-1 < ρ < 1)
    pub rho: f64,
    /// Horizontal shift (ATM location in log-moneyness)
    pub m: f64,
    /// Smoothness/ATM curvature (σ > 0)
    pub sigma: f64,
}

impl SVIParams {
    /// Create new SVI parameters
    pub fn new(a: f64, b: f64, rho: f64, m: f64, sigma: f64) -> Self {
        Self { a, b, rho, m, sigma }
    }

    /// Check Gatheral & Jacquier no-arbitrage constraints
    ///
    /// For butterfly arbitrage-free:
    /// 1. b >= 0 (positive wing slope)
    /// 2. |ρ| < 1 (valid correlation)
    /// 3. σ > 0 (positive curvature)
    /// 4. a + b × σ × √(1 - ρ²) >= 0 (non-negative variance at wings)
    pub fn is_valid(&self) -> bool {
        self.b >= 0.0
            && self.rho.abs() < 1.0
            && self.sigma > 0.0
            && self.a + self.b * self.sigma * (1.0 - self.rho.powi(2)).sqrt() >= 0.0
    }

    /// Compute total variance at log-moneyness k
    pub fn total_variance(&self, k: f64) -> f64 {
        let x = k - self.m;
        self.a + self.b * (self.rho * x + (x * x + self.sigma * self.sigma).sqrt())
    }

    /// Compute IV at log-moneyness k, given time to expiry
    pub fn iv(&self, k: f64, tte: f64) -> f64 {
        let w = self.total_variance(k);
        if w > 0.0 && tte > 0.0 {
            (w / tte).sqrt()
        } else {
            0.0
        }
    }

    /// Compute first derivative dw/dk (for delta/gamma)
    pub fn dw_dk(&self, k: f64) -> f64 {
        let x = k - self.m;
        let sqrt_term = (x * x + self.sigma * self.sigma).sqrt();
        self.b * (self.rho + x / sqrt_term)
    }

    /// Compute second derivative d²w/dk² (for butterfly arbitrage check)
    pub fn d2w_dk2(&self, k: f64) -> f64 {
        let x = k - self.m;
        let sigma_sq = self.sigma * self.sigma;
        let sqrt_term = (x * x + sigma_sq).sqrt();
        self.b * sigma_sq / (sqrt_term * sqrt_term * sqrt_term)
    }

    /// Check local butterfly arbitrage at a given k
    ///
    /// Butterfly arbitrage exists if d²w/dk² < 0 (non-convex)
    pub fn has_butterfly_arbitrage(&self, k: f64) -> bool {
        self.d2w_dk2(k) < -1e-10
    }

    /// Get ATM total variance (at k = 0)
    pub fn atm_variance(&self) -> f64 {
        self.total_variance(0.0)
    }

    /// Get ATM IV given time to expiry
    pub fn atm_iv(&self, tte: f64) -> f64 {
        self.iv(0.0, tte)
    }

    /// Convert log-moneyness to strike given forward price
    pub fn k_to_strike(k: f64, forward: f64) -> f64 {
        forward * k.exp()
    }

    /// Convert strike to log-moneyness given forward price
    pub fn strike_to_k(strike: f64, forward: f64) -> f64 {
        (strike / forward).ln()
    }
}

impl Default for SVIParams {
    /// Typical equity skew defaults
    fn default() -> Self {
        Self {
            a: 0.04,    // Base variance ~20% vol
            b: 0.1,     // Moderate wing slope
            rho: -0.3,  // Typical negative skew
            m: 0.0,     // ATM centered
            sigma: 0.1, // Moderate curvature
        }
    }
}

/// Error types for SVI operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum SVIError {
    #[error("Insufficient data points: need at least 5, got {0}")]
    InsufficientData(usize),
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
    #[error("Optimization failed: {0}")]
    OptimizationFailed(String),
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_svi_default_is_valid() {
        let params = SVIParams::default();
        assert!(params.is_valid());
    }

    #[test]
    fn test_svi_total_variance_atm() {
        let params = SVIParams::new(0.04, 0.1, -0.3, 0.0, 0.1);
        let w = params.total_variance(0.0);
        // At k=0: w = a + b * (ρ*0 + √(0 + σ²)) = a + b*σ
        let expected = 0.04 + 0.1 * 0.1;
        assert_relative_eq!(w, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_svi_symmetry_with_zero_rho() {
        let params = SVIParams::new(0.04, 0.1, 0.0, 0.0, 0.1);
        let w_pos = params.total_variance(0.1);
        let w_neg = params.total_variance(-0.1);
        // With ρ=0, smile should be symmetric
        assert_relative_eq!(w_pos, w_neg, epsilon = 1e-10);
    }

    #[test]
    fn test_svi_skew_with_negative_rho() {
        let params = SVIParams::new(0.04, 0.1, -0.5, 0.0, 0.1);
        let w_otm_put = params.total_variance(-0.1);  // OTM put
        let w_otm_call = params.total_variance(0.1);  // OTM call
        // Negative rho means higher variance on downside (puts)
        assert!(w_otm_put > w_otm_call);
    }

    #[test]
    fn test_svi_iv_from_variance() {
        let params = SVIParams::new(0.04, 0.1, -0.3, 0.0, 0.1);
        let tte = 0.25; // 3 months
        let iv = params.iv(0.0, tte);
        let w = params.total_variance(0.0);
        assert_relative_eq!(iv, (w / tte).sqrt(), epsilon = 1e-10);
    }

    #[test]
    fn test_svi_constraint_violation_negative_b() {
        let params = SVIParams::new(0.04, -0.1, -0.3, 0.0, 0.1);
        assert!(!params.is_valid());
    }

    #[test]
    fn test_svi_constraint_violation_rho_out_of_range() {
        let params = SVIParams::new(0.04, 0.1, 1.5, 0.0, 0.1);
        assert!(!params.is_valid());
    }

    #[test]
    fn test_svi_constraint_violation_negative_sigma() {
        let params = SVIParams::new(0.04, 0.1, -0.3, 0.0, -0.1);
        assert!(!params.is_valid());
    }

    #[test]
    fn test_svi_second_derivative_positive() {
        let params = SVIParams::default();
        // Valid SVI should have positive second derivative everywhere
        for k in [-0.5, -0.2, 0.0, 0.2, 0.5] {
            assert!(params.d2w_dk2(k) > 0.0, "d2w/dk2 should be positive at k={}", k);
        }
    }

    #[test]
    fn test_svi_no_butterfly_arbitrage() {
        let params = SVIParams::default();
        for k in [-0.5, -0.2, 0.0, 0.2, 0.5] {
            assert!(!params.has_butterfly_arbitrage(k));
        }
    }

    #[test]
    fn test_strike_log_moneyness_roundtrip() {
        let forward = 100.0;
        let strike = 95.0;
        let k = SVIParams::strike_to_k(strike, forward);
        let strike_back = SVIParams::k_to_strike(k, forward);
        assert_relative_eq!(strike, strike_back, epsilon = 1e-10);
    }
}
