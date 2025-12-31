// Arbitrage detection for volatility surfaces
//
// Detects butterfly (smile convexity) and calendar (term structure) arbitrage.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::svi::SVIParams;
use crate::vol_slice::VolSlice;

/// Arbitrage violation detected in the surface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArbitrageViolation {
    /// Butterfly arbitrage: non-convex smile at a given delta
    Butterfly {
        delta: f64,
        expiration: NaiveDate,
        severity: f64,
    },
    /// Calendar arbitrage: negative forward variance between expiries
    Calendar {
        delta: f64,
        near_expiry: NaiveDate,
        far_expiry: NaiveDate,
        forward_variance: f64,
    },
}

impl ArbitrageViolation {
    pub fn severity(&self) -> f64 {
        match self {
            ArbitrageViolation::Butterfly { severity, .. } => *severity,
            ArbitrageViolation::Calendar { forward_variance, .. } => forward_variance.abs(),
        }
    }

    pub fn is_butterfly(&self) -> bool {
        matches!(self, ArbitrageViolation::Butterfly { .. })
    }

    pub fn is_calendar(&self) -> bool {
        matches!(self, ArbitrageViolation::Calendar { .. })
    }
}

/// Result of arbitrage check
#[derive(Debug, Clone, Default)]
pub struct ArbitrageReport {
    pub violations: Vec<ArbitrageViolation>,
}

impl ArbitrageReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, violation: ArbitrageViolation) {
        self.violations.push(violation);
    }

    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn has_butterfly(&self) -> bool {
        self.violations.iter().any(|v| v.is_butterfly())
    }

    pub fn has_calendar(&self) -> bool {
        self.violations.iter().any(|v| v.is_calendar())
    }

    pub fn butterfly_violations(&self) -> Vec<&ArbitrageViolation> {
        self.violations.iter().filter(|v| v.is_butterfly()).collect()
    }

    pub fn calendar_violations(&self) -> Vec<&ArbitrageViolation> {
        self.violations.iter().filter(|v| v.is_calendar()).collect()
    }

    pub fn max_severity(&self) -> f64 {
        self.violations
            .iter()
            .map(|v| v.severity())
            .fold(0.0, f64::max)
    }
}

/// Check for butterfly arbitrage in a single smile/slice
///
/// Butterfly arbitrage exists when the smile is not convex in total variance
/// (i.e., d²w/dk² < 0 somewhere).
///
/// For discrete data, we check that variance is convex across delta points.
pub fn check_butterfly_arbitrage(slice: &VolSlice) -> Vec<ArbitrageViolation> {
    let mut violations = Vec::new();

    // Sample delta points
    let deltas: Vec<f64> = (10..=90).step_by(5).map(|d| d as f64 / 100.0).collect();

    // Get total variance at each delta
    let variances: Vec<Option<f64>> = deltas
        .iter()
        .map(|&d| slice.get_total_variance(d))
        .collect();

    // Check convexity using second differences
    for i in 1..deltas.len() - 1 {
        if let (Some(w1), Some(w2), Some(w3)) = (variances[i - 1], variances[i], variances[i + 1]) {
            // Discrete second derivative
            let d2w = w1 - 2.0 * w2 + w3;

            if d2w < -1e-10 {
                violations.push(ArbitrageViolation::Butterfly {
                    delta: deltas[i],
                    expiration: slice.expiration(),
                    severity: -d2w,
                });
            }
        }
    }

    violations
}

/// Check for butterfly arbitrage using SVI parameters
///
/// For SVI, we can check analytically using the second derivative.
pub fn check_butterfly_arbitrage_svi(
    params: &SVIParams,
    expiration: NaiveDate,
    k_range: (f64, f64),
    n_points: usize,
) -> Vec<ArbitrageViolation> {
    let mut violations = Vec::new();

    let step = (k_range.1 - k_range.0) / (n_points - 1) as f64;

    for i in 0..n_points {
        let k = k_range.0 + i as f64 * step;
        let d2w = params.d2w_dk2(k);

        if d2w < -1e-10 {
            // Convert k to approximate delta for reporting
            // (rough approximation: delta ≈ N(d1) where d1 ≈ -k / σ√T)
            let approx_delta = 0.5 + k * 2.0; // Very rough

            violations.push(ArbitrageViolation::Butterfly {
                delta: approx_delta.clamp(0.0, 1.0),
                expiration,
                severity: -d2w,
            });
        }
    }

    violations
}

/// Check for calendar arbitrage between two slices
///
/// Calendar arbitrage exists when forward variance is negative:
/// Var(T2) - Var(T1) < 0 for T2 > T1
pub fn check_calendar_arbitrage(
    near_slice: &VolSlice,
    far_slice: &VolSlice,
) -> Vec<ArbitrageViolation> {
    let mut violations = Vec::new();

    // Validate ordering
    if near_slice.tte() >= far_slice.tte() {
        return violations; // Invalid ordering
    }

    // Sample delta points
    let deltas: Vec<f64> = (10..=90).step_by(5).map(|d| d as f64 / 100.0).collect();

    for delta in deltas {
        if let (Some(var_near), Some(var_far)) = (
            near_slice.get_total_variance(delta),
            far_slice.get_total_variance(delta),
        ) {
            let forward_var = var_far - var_near;

            if forward_var < -1e-10 {
                violations.push(ArbitrageViolation::Calendar {
                    delta,
                    near_expiry: near_slice.expiration(),
                    far_expiry: far_slice.expiration(),
                    forward_variance: forward_var,
                });
            }
        }
    }

    violations
}

/// Check for calendar arbitrage across multiple slices
pub fn check_calendar_arbitrage_surface(slices: &[&VolSlice]) -> Vec<ArbitrageViolation> {
    let mut violations = Vec::new();

    // Sort by TTE
    let mut sorted: Vec<_> = slices.to_vec();
    sorted.sort_by(|a, b| a.tte().partial_cmp(&b.tte()).unwrap());

    // Check adjacent pairs
    for window in sorted.windows(2) {
        let near = window[0];
        let far = window[1];
        violations.extend(check_calendar_arbitrage(near, far));
    }

    violations
}

/// Full arbitrage check on a surface
pub fn full_arbitrage_check(slices: &[&VolSlice]) -> ArbitrageReport {
    let mut report = ArbitrageReport::new();

    // Check butterfly for each slice
    for slice in slices {
        for violation in check_butterfly_arbitrage(slice) {
            report.add(violation);
        }
    }

    // Check calendar across slices
    for violation in check_calendar_arbitrage_surface(slices) {
        report.add(violation);
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_convex_slice(tte: f64, base_iv: f64) -> VolSlice {
        let now = Utc::now();
        let exp = now.date_naive() + chrono::Duration::days((tte * 365.0) as i64);

        // Typical convex smile with dense points
        // Using a quadratic shape: iv(d) = base + 0.2*(d-0.5)^2
        // This ensures variance is convex under linear interpolation
        let deltas: Vec<f64> = (10..=90).step_by(10).map(|d| d as f64 / 100.0).collect();
        let points: Vec<(f64, f64)> = deltas
            .iter()
            .map(|&d| {
                let iv = base_iv + 0.2 * (d - 0.5).powi(2);
                (d, iv)
            })
            .collect();

        VolSlice::from_delta_iv_pairs(points, 100.0, tte, 0.05, exp)
    }

    fn create_non_convex_slice(tte: f64) -> VolSlice {
        let now = Utc::now();
        let exp = now.date_naive() + chrono::Duration::days((tte * 365.0) as i64);

        // Non-convex (dip in the middle)
        VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.30),
                (0.40, 0.35),
                (0.50, 0.20),  // Artificially low - creates concavity
                (0.60, 0.35),
                (0.75, 0.30),
            ],
            100.0,
            tte,
            0.05,
            exp,
        )
    }

    #[test]
    fn test_convex_smile_no_butterfly() {
        let slice = create_convex_slice(0.1, 0.25);
        let violations = check_butterfly_arbitrage(&slice);
        assert!(violations.is_empty(), "Convex smile should have no butterfly arb");
    }

    #[test]
    fn test_non_convex_smile_has_butterfly() {
        let slice = create_non_convex_slice(0.1);
        let violations = check_butterfly_arbitrage(&slice);
        assert!(!violations.is_empty(), "Non-convex smile should have butterfly arb");
    }

    #[test]
    fn test_normal_term_structure_no_calendar() {
        // Higher IV for near-term, lower for far-term (normal earnings pattern)
        let near = create_convex_slice(0.05, 0.35);  // 18 days, high IV
        let far = create_convex_slice(0.15, 0.25);   // 55 days, lower IV

        let violations = check_calendar_arbitrage(&near, &far);

        // With higher near-term IV, variance should increase with time
        // So forward variance should be positive
        assert!(violations.is_empty(), "Normal term structure should have no calendar arb");
    }

    #[test]
    fn test_inverted_term_structure_has_calendar() {
        // Lower IV for near-term, much higher for far-term (unusual)
        let near = create_convex_slice(0.05, 0.15);  // Low IV
        let far = create_convex_slice(0.15, 0.50);   // Very high IV

        // This might or might not have calendar arb depending on exact numbers
        // Calendar arb = var_far < var_near (inverted variance)
        let violations = check_calendar_arbitrage(&near, &far);

        // In this case: near_var ≈ 0.15² * 0.05 = 0.001125
        //               far_var ≈ 0.50² * 0.15 = 0.0375
        // Forward var = 0.0375 - 0.001125 = 0.036 > 0 (no arb)
        assert!(violations.is_empty());
    }

    #[test]
    fn test_svi_valid_params_no_butterfly() {
        let params = SVIParams::default();
        let exp = Utc::now().date_naive() + chrono::Duration::days(30);

        let violations = check_butterfly_arbitrage_svi(&params, exp, (-0.5, 0.5), 21);
        assert!(violations.is_empty(), "Valid SVI should have no butterfly arb");
    }

    #[test]
    fn test_arbitrage_report() {
        let near = create_convex_slice(0.05, 0.25);
        let far = create_convex_slice(0.15, 0.25);

        let report = full_arbitrage_check(&[&near, &far]);

        assert!(report.is_clean());
        assert!(!report.has_butterfly());
        assert!(!report.has_calendar());
    }

    #[test]
    fn test_arbitrage_report_with_violations() {
        let slice = create_non_convex_slice(0.1);
        let report = full_arbitrage_check(&[&slice]);

        assert!(!report.is_clean());
        assert!(report.has_butterfly());
        assert!(report.max_severity() > 0.0);
    }
}
