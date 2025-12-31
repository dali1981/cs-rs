// Mathematical utility functions for options analytics

/// Inverse of standard normal CDF (quantile function)
/// Uses Abramowitz & Stegun approximation (rational approximation)
///
/// # Arguments
/// * `p` - Probability value in (0, 1)
///
/// # Returns
/// The z-score such that N(z) = p
pub fn inv_norm_cdf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }

    // Coefficients for rational approximation
    const A: [f64; 6] = [
        -3.969683028665376e+01,
         2.209460984245205e+02,
        -2.759285104469687e+02,
         1.383577518672690e+02,
        -3.066479806614716e+01,
         2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
         1.615858368580409e+02,
        -1.556989798598866e+02,
         6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
         4.374664141464968e+00,
         2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
         7.784695709041462e-03,
         3.224671290700398e-01,
         2.445134137142996e+00,
         3.754408661907416e+00,
    ];

    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    if p < P_LOW {
        // Lower tail
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0]*q + C[1])*q + C[2])*q + C[3])*q + C[4])*q + C[5]) /
        ((((D[0]*q + D[1])*q + D[2])*q + D[3])*q + 1.0)
    } else if p <= P_HIGH {
        // Central region
        let q = p - 0.5;
        let r = q * q;
        (((((A[0]*r + A[1])*r + A[2])*r + A[3])*r + A[4])*r + A[5]) * q /
        (((((B[0]*r + B[1])*r + B[2])*r + B[3])*r + B[4])*r + 1.0)
    } else {
        // Upper tail
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0]*q + C[1])*q + C[2])*q + C[3])*q + C[4])*q + C[5]) /
        ((((D[0]*q + D[1])*q + D[2])*q + D[3])*q + 1.0)
    }
}

/// Linearly space `n` values from `start` to `end` (inclusive)
pub fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return vec![];
    }
    if n == 1 {
        return vec![start];
    }
    let step = (end - start) / (n - 1) as f64;
    (0..n).map(|i| start + i as f64 * step).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use statrs::distribution::{ContinuousCDF, Normal};

    #[test]
    fn test_inv_norm_cdf_median() {
        // N^-1(0.5) = 0
        let result = inv_norm_cdf(0.5);
        assert_relative_eq!(result, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_inv_norm_cdf_standard_points() {
        // Test against known values
        // N^-1(0.025) ≈ -1.96
        // N^-1(0.05) ≈ -1.645
        // N^-1(0.10) ≈ -1.282
        // N^-1(0.50) = 0
        // N^-1(0.90) ≈ 1.282
        // N^-1(0.95) ≈ 1.645
        // N^-1(0.975) ≈ 1.96

        assert_relative_eq!(inv_norm_cdf(0.025), -1.96, epsilon = 0.01);
        assert_relative_eq!(inv_norm_cdf(0.05), -1.645, epsilon = 0.01);
        assert_relative_eq!(inv_norm_cdf(0.10), -1.282, epsilon = 0.01);
        assert_relative_eq!(inv_norm_cdf(0.50), 0.0, epsilon = 0.01);
        assert_relative_eq!(inv_norm_cdf(0.90), 1.282, epsilon = 0.01);
        assert_relative_eq!(inv_norm_cdf(0.95), 1.645, epsilon = 0.01);
        assert_relative_eq!(inv_norm_cdf(0.975), 1.96, epsilon = 0.01);
    }

    #[test]
    fn test_inv_norm_cdf_roundtrip() {
        let norm = Normal::new(0.0, 1.0).unwrap();

        // Test roundtrip: N(N^-1(p)) = p
        for p in [0.01, 0.10, 0.25, 0.50, 0.75, 0.90, 0.99] {
            let z = inv_norm_cdf(p);
            let p_back = norm.cdf(z);
            assert_relative_eq!(p_back, p, epsilon = 1e-6);
        }
    }

    #[test]
    fn test_inv_norm_cdf_boundaries() {
        assert!(inv_norm_cdf(0.0).is_infinite() && inv_norm_cdf(0.0) < 0.0);
        assert!(inv_norm_cdf(1.0).is_infinite() && inv_norm_cdf(1.0) > 0.0);
    }

    #[test]
    fn test_linspace_basic() {
        let result = linspace(0.0, 1.0, 5);
        assert_eq!(result.len(), 5);
        assert_relative_eq!(result[0], 0.0, epsilon = 1e-10);
        assert_relative_eq!(result[1], 0.25, epsilon = 1e-10);
        assert_relative_eq!(result[2], 0.5, epsilon = 1e-10);
        assert_relative_eq!(result[3], 0.75, epsilon = 1e-10);
        assert_relative_eq!(result[4], 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_linspace_single() {
        let result = linspace(5.0, 10.0, 1);
        assert_eq!(result.len(), 1);
        assert_relative_eq!(result[0], 5.0, epsilon = 1e-10);
    }

    #[test]
    fn test_linspace_empty() {
        let result = linspace(0.0, 1.0, 0);
        assert!(result.is_empty());
    }
}
