// Single expiry volatility smile, parameterized by delta

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::black_scholes::bs_delta;
use crate::math_utils::inv_norm_cdf;
use crate::svi::{SVIError, SVIParams};
use crate::svi_fitter::SVIFitter;

/// Interpolation mode for volatility slices
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMode {
    /// Linear interpolation in delta-space (M1 default)
    #[default]
    Linear,
    /// SVI parametric fit (M2)
    SVI,
}

impl InterpolationMode {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "svi" => Self::SVI,
            _ => Self::Linear,
        }
    }
}

/// A single expiry's volatility smile, parameterized by delta.
///
/// This structure represents the IV smile for a single expiration date,
/// indexed by Black-Scholes delta rather than strike price. This is
/// particularly useful for sticky-delta modeling around earnings events.
///
/// Supports multiple interpolation modes:
/// - Linear: Simple linear interpolation between delta points (M1 default)
/// - SVI: Parametric SVI fit for smoother extrapolation (M2 opt-in)
#[derive(Debug, Clone)]
pub struct VolSlice {
    /// Expiration date
    expiration: NaiveDate,
    /// Time to expiry in years
    tte: f64,
    /// Delta → IV mapping, sorted by delta ascending
    /// Uses Vec instead of BTreeMap to avoid OrderedFloat dependency
    smile: Vec<(f64, f64)>,
    /// Reference spot price
    spot: f64,
    /// Risk-free rate used for delta calculations
    risk_free_rate: f64,
    /// Forward price for log-moneyness calculations (SVI)
    forward: f64,
    /// Interpolation mode
    mode: InterpolationMode,
    /// SVI parameters (if fitted)
    svi_params: Option<SVIParams>,
}

impl VolSlice {
    /// Build from market data points (strike, iv) pairs
    ///
    /// Converts strike-space quotes to delta-space by computing Black-Scholes delta.
    pub fn from_points(
        points: &[(f64, f64)],  // (strike, iv) pairs
        spot: f64,
        tte: f64,
        risk_free_rate: f64,
        expiration: NaiveDate,
    ) -> Self {
        let mut smile: Vec<(f64, f64)> = points
            .iter()
            .filter_map(|&(strike, iv)| {
                if iv <= 0.0 || strike <= 0.0 || tte <= 0.0 {
                    return None;
                }
                // Compute call delta for this strike/iv
                let delta = bs_delta(spot, strike, tte, iv, true, risk_free_rate);
                Some((delta, iv))
            })
            .collect();

        // Sort by delta
        smile.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Forward price: F = S * e^(r*T)
        let forward = spot * (risk_free_rate * tte).exp();

        Self {
            expiration,
            tte,
            smile,
            spot,
            risk_free_rate,
            forward,
            mode: InterpolationMode::Linear,
            svi_params: None,
        }
    }

    /// Create from pre-computed delta-iv pairs
    pub fn from_delta_iv_pairs(
        pairs: Vec<(f64, f64)>,
        spot: f64,
        tte: f64,
        risk_free_rate: f64,
        expiration: NaiveDate,
    ) -> Self {
        let mut smile = pairs;
        smile.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Forward price: F = S * e^(r*T)
        let forward = spot * (risk_free_rate * tte).exp();

        Self {
            expiration,
            tte,
            smile,
            spot,
            risk_free_rate,
            forward,
            mode: InterpolationMode::Linear,
            svi_params: None,
        }
    }

    /// Set interpolation mode
    pub fn with_mode(mut self, mode: InterpolationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Get the current interpolation mode
    pub fn mode(&self) -> InterpolationMode {
        self.mode
    }

    /// Get SVI parameters if fitted
    pub fn svi_params(&self) -> Option<&SVIParams> {
        self.svi_params.as_ref()
    }

    /// Fit SVI parameters to the smile data
    ///
    /// Converts (delta, iv) pairs to (log_moneyness, total_variance) and fits SVI.
    /// After fitting, the slice will use SVI interpolation.
    pub fn fit_svi(&mut self) -> Result<(), SVIError> {
        if self.smile.len() < 5 {
            return Err(SVIError::InsufficientData(self.smile.len()));
        }

        // Convert (delta, iv) to (log_moneyness, total_variance)
        let data: Vec<(f64, f64)> = self.smile
            .iter()
            .filter_map(|&(delta, iv)| {
                // Get strike from delta
                let strike = self.delta_to_strike_internal(delta)?;
                // Log-moneyness
                let k = (strike / self.forward).ln();
                // Total variance
                let w = iv * iv * self.tte;
                Some((k, w))
            })
            .collect();

        if data.len() < 5 {
            return Err(SVIError::InsufficientData(data.len()));
        }

        let fitter = SVIFitter::new();
        self.svi_params = Some(fitter.fit(&data)?);
        self.mode = InterpolationMode::SVI;

        Ok(())
    }

    /// Internal delta to strike conversion (doesn't check mode)
    fn delta_to_strike_internal(&self, delta: f64) -> Option<f64> {
        // Use linear interpolation to get IV at this delta
        let iv = self.linear_interp(delta)?;
        delta_to_strike_with_iv(delta, iv, self.spot, self.tte, self.risk_free_rate, true)
    }

    pub fn expiration(&self) -> NaiveDate {
        self.expiration
    }

    pub fn tte(&self) -> f64 {
        self.tte
    }

    pub fn spot(&self) -> f64 {
        self.spot
    }

    pub fn risk_free_rate(&self) -> f64 {
        self.risk_free_rate
    }

    /// Get the raw smile data (delta, iv) pairs
    pub fn smile_points(&self) -> &[(f64, f64)] {
        &self.smile
    }

    /// Interpolate IV at a target delta
    ///
    /// Uses the configured interpolation mode (Linear or SVI).
    ///
    /// # Arguments
    /// * `target_delta` - The call delta to interpolate at (0 to 1)
    ///
    /// # Returns
    /// The interpolated IV, or None if the slice is empty
    pub fn get_iv(&self, target_delta: f64) -> Option<f64> {
        match self.mode {
            InterpolationMode::Linear => self.linear_interp(target_delta),
            InterpolationMode::SVI => self.svi_interp(target_delta),
        }
    }

    /// Linear interpolation in delta-space (M1)
    fn linear_interp(&self, target_delta: f64) -> Option<f64> {
        if self.smile.is_empty() {
            return None;
        }

        // Find bracketing points
        let mut lower: Option<(f64, f64)> = None;
        let mut upper: Option<(f64, f64)> = None;

        for &(delta, iv) in &self.smile {
            if delta <= target_delta {
                lower = Some((delta, iv));
            } else if upper.is_none() {
                upper = Some((delta, iv));
                break;
            }
        }

        match (lower, upper) {
            (Some((d1, iv1)), Some((d2, iv2))) => {
                // Linear interpolation
                let weight = (target_delta - d1) / (d2 - d1);
                Some(iv1 + weight * (iv2 - iv1))
            }
            (Some((_, iv)), None) => Some(iv),  // Extrapolate flat (use highest delta point)
            (None, Some((_, iv))) => Some(iv),  // Extrapolate flat (use lowest delta point)
            (None, None) => None,
        }
    }

    /// SVI interpolation via parametric model (M2)
    fn svi_interp(&self, target_delta: f64) -> Option<f64> {
        let params = self.svi_params.as_ref()?;

        // Convert delta to strike, then to log-moneyness
        let strike = self.delta_to_strike_internal(target_delta)?;
        let k = (strike / self.forward).ln();

        // Get IV from SVI
        Some(params.iv(k, self.tte))
    }

    /// Get total variance at a delta (for time interpolation)
    ///
    /// Total variance = σ² × τ, which is additive in time and
    /// preferred for interpolation in the term structure.
    pub fn get_total_variance(&self, delta: f64) -> Option<f64> {
        self.get_iv(delta).map(|iv| iv * iv * self.tte)
    }

    /// Map delta back to strike
    ///
    /// # Arguments
    /// * `delta` - The call delta (0 to 1) to convert
    /// * `is_call` - Whether to compute call or put strike
    ///
    /// # Returns
    /// The strike price corresponding to this delta, or None if IV unavailable
    pub fn delta_to_strike(&self, delta: f64, is_call: bool) -> Option<f64> {
        let iv = self.get_iv(delta)?;
        delta_to_strike_with_iv(
            delta,
            iv,
            self.spot,
            self.tte,
            self.risk_free_rate,
            is_call,
        )
    }

    /// Get IV at a specific strike (reverse lookup from delta-space)
    ///
    /// This converts the delta-space smile to strike-space and interpolates.
    /// Used by strike-space selection model to compare IVs at the same strike.
    ///
    /// # Arguments
    /// * `target_strike` - The strike price to get IV for
    ///
    /// # Returns
    /// The interpolated IV at that strike, or None if unavailable
    pub fn get_iv_at_strike(&self, target_strike: f64) -> Option<f64> {
        if self.smile.is_empty() || target_strike <= 0.0 {
            return None;
        }

        // Convert delta-space smile to strike-space
        // For each (delta, iv) point, compute corresponding strike
        let mut strike_iv_pairs: Vec<(f64, f64)> = self
            .smile
            .iter()
            .filter_map(|&(delta, iv)| {
                // Compute strike for this delta using this IV
                let strike = delta_to_strike_with_iv(
                    delta,
                    iv,
                    self.spot,
                    self.tte,
                    self.risk_free_rate,
                    true,  // Use call delta consistently
                )?;
                Some((strike, iv))
            })
            .collect();

        if strike_iv_pairs.is_empty() {
            return None;
        }

        // Sort by strike
        strike_iv_pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Find bracketing points
        let mut lower: Option<(f64, f64)> = None;
        let mut upper: Option<(f64, f64)> = None;

        for &(strike, iv) in &strike_iv_pairs {
            if strike <= target_strike {
                lower = Some((strike, iv));
            } else if upper.is_none() {
                upper = Some((strike, iv));
                break;
            }
        }

        // Interpolate in strike-space
        match (lower, upper) {
            (Some((k1, iv1)), Some((k2, iv2))) => {
                // Linear interpolation
                let weight = (target_strike - k1) / (k2 - k1);
                Some(iv1 + weight * (iv2 - iv1))
            }
            (Some((_, iv)), None) => Some(iv),  // Extrapolate flat (use highest strike)
            (None, Some((_, iv))) => Some(iv),  // Extrapolate flat (use lowest strike)
            (None, None) => None,
        }
    }
}

/// Convert delta to strike using Black-Scholes inversion
///
/// For calls: Δ = N(d1), so d1 = N⁻¹(Δ)
/// d1 = [ln(S/K) + (r + σ²/2)T] / (σ√T)
/// Solve for K:
/// ln(S/K) = d1 * σ√T - (r + σ²/2)T
/// K = S * exp(-(d1 * σ√T - (r + σ²/2)T))
pub fn delta_to_strike_with_iv(
    delta: f64,
    iv: f64,
    spot: f64,
    tte: f64,
    risk_free_rate: f64,
    is_call: bool,
) -> Option<f64> {
    if iv <= 0.0 || tte <= 0.0 || spot <= 0.0 {
        return None;
    }

    // For calls: Δ = N(d1), so d1 = N⁻¹(Δ)
    // For puts: Δ = N(d1) - 1, so d1 = N⁻¹(Δ + 1)
    let d1 = if is_call {
        inv_norm_cdf(delta)
    } else {
        inv_norm_cdf(delta + 1.0)
    };

    if d1.is_infinite() {
        return None;
    }

    let sqrt_t = tte.sqrt();
    let exponent = -(d1 * iv * sqrt_t - (risk_free_rate + 0.5 * iv * iv) * tte);

    Some(spot * exponent.exp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn create_test_slice() -> VolSlice {
        // Create a simple smile with 5 points
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        // Typical smile: higher IV for OTM puts (low delta), lower for ATM
        let points = vec![
            (90.0, 0.35),   // OTM put → low call delta
            (95.0, 0.30),   // Slightly OTM put
            (100.0, 0.25),  // ATM
            (105.0, 0.28),  // Slightly OTM call
            (110.0, 0.32),  // OTM call → high call delta
        ];

        VolSlice::from_points(&points, spot, tte, rfr, expiration)
    }

    #[test]
    fn test_vol_slice_from_points() {
        let slice = create_test_slice();

        // Should have 5 points
        assert_eq!(slice.smile_points().len(), 5);

        // Points should be sorted by delta
        let points = slice.smile_points();
        for i in 1..points.len() {
            assert!(points[i].0 >= points[i-1].0, "Smile should be sorted by delta");
        }
    }

    #[test]
    fn test_vol_slice_get_iv_exact() {
        let slice = create_test_slice();

        // Get IV at one of the existing deltas
        let points = slice.smile_points();
        let (delta, expected_iv) = points[2]; // Middle point

        let iv = slice.get_iv(delta);
        assert!(iv.is_some());
        assert_relative_eq!(iv.unwrap(), expected_iv, epsilon = 1e-10);
    }

    #[test]
    fn test_vol_slice_get_iv_interpolated() {
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        // Create simple two-point smile for predictable interpolation
        let pairs = vec![
            (0.25, 0.30),
            (0.75, 0.20),
        ];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        // Interpolate at midpoint
        let iv = slice.get_iv(0.50);
        assert!(iv.is_some());
        assert_relative_eq!(iv.unwrap(), 0.25, epsilon = 1e-10);
    }

    #[test]
    fn test_vol_slice_get_iv_extrapolate_flat() {
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        let pairs = vec![
            (0.25, 0.30),
            (0.75, 0.20),
        ];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        // Below range: should extrapolate flat using lowest point
        let iv_low = slice.get_iv(0.10);
        assert!(iv_low.is_some());
        assert_relative_eq!(iv_low.unwrap(), 0.30, epsilon = 1e-10);

        // Above range: should extrapolate flat using highest point
        let iv_high = slice.get_iv(0.90);
        assert!(iv_high.is_some());
        assert_relative_eq!(iv_high.unwrap(), 0.20, epsilon = 1e-10);
    }

    #[test]
    fn test_vol_slice_total_variance() {
        let spot = 100.0;
        let tte = 0.25; // 3 months
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        let pairs = vec![(0.50, 0.20)];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        let var = slice.get_total_variance(0.50);
        assert!(var.is_some());
        // IV = 0.20, tte = 0.25, so total var = 0.20^2 * 0.25 = 0.01
        assert_relative_eq!(var.unwrap(), 0.01, epsilon = 1e-10);
    }

    #[test]
    fn test_vol_slice_delta_to_strike_roundtrip() {
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        // ATM point
        let pairs = vec![(0.50, 0.25)];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        // Convert delta to strike
        let strike = slice.delta_to_strike(0.50, true);
        assert!(strike.is_some());

        let strike = strike.unwrap();
        // For 50 delta, strike should be close to spot (slightly adjusted for drift)
        assert!((strike - spot).abs() < 5.0, "50 delta strike {} should be near spot {}", strike, spot);
    }

    #[test]
    fn test_vol_slice_empty() {
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();
        let slice = VolSlice::from_points(&[], 100.0, 0.1, 0.05, expiration);

        assert!(slice.get_iv(0.50).is_none());
        assert!(slice.get_total_variance(0.50).is_none());
        assert!(slice.delta_to_strike(0.50, true).is_none());
    }

    #[test]
    fn test_delta_to_strike_with_iv() {
        let spot = 100.0;
        let tte = 0.25;
        let rfr = 0.05;
        let iv = 0.20;

        // Test call delta to strike
        let strike = delta_to_strike_with_iv(0.50, iv, spot, tte, rfr, true);
        assert!(strike.is_some());
        let strike = strike.unwrap();

        // Verify by computing delta of resulting strike
        let computed_delta = bs_delta(spot, strike, tte, iv, true, rfr);
        assert_relative_eq!(computed_delta, 0.50, epsilon = 0.01);
    }

    #[test]
    fn test_delta_to_strike_with_iv_put() {
        let spot = 100.0;
        let tte = 0.25;
        let rfr = 0.05;
        let iv = 0.20;

        // -0.25 delta put
        let strike = delta_to_strike_with_iv(-0.25, iv, spot, tte, rfr, false);
        assert!(strike.is_some());
        let strike = strike.unwrap();

        // Put strike should be below spot for OTM put
        assert!(strike < spot, "OTM put strike {} should be below spot {}", strike, spot);

        // Verify by computing delta of resulting strike
        let computed_delta = bs_delta(spot, strike, tte, iv, false, rfr);
        assert_relative_eq!(computed_delta, -0.25, epsilon = 0.02);
    }

    #[test]
    fn test_vol_slice_get_iv_at_strike_exact() {
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        // Create smile with known delta-IV pairs
        let pairs = vec![
            (0.25, 0.35),  // Low delta (OTM put) → high IV
            (0.50, 0.25),  // ATM → medium IV
            (0.75, 0.30),  // High delta (ITM call) → slightly higher IV
        ];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        // Get strike for 50 delta
        let strike_50d = slice.delta_to_strike(0.50, true).unwrap();

        // Get IV at that strike - should match original IV
        let iv_at_strike = slice.get_iv_at_strike(strike_50d);
        assert!(iv_at_strike.is_some());
        assert_relative_eq!(iv_at_strike.unwrap(), 0.25, epsilon = 0.01);
    }

    #[test]
    fn test_vol_slice_get_iv_at_strike_interpolated() {
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        // Create smile with known delta-IV pairs
        let pairs = vec![
            (0.25, 0.35),
            (0.75, 0.25),
        ];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        // Get strikes for the delta points
        let strike_25d = slice.delta_to_strike(0.25, true).unwrap();
        let strike_75d = slice.delta_to_strike(0.75, true).unwrap();

        // Get IV at a strike between them
        let mid_strike = (strike_25d + strike_75d) / 2.0;
        let iv_at_mid = slice.get_iv_at_strike(mid_strike);
        assert!(iv_at_mid.is_some());

        // Should be between 0.25 and 0.35
        let iv = iv_at_mid.unwrap();
        assert!(iv >= 0.24 && iv <= 0.36, "IV {} should be in range [0.24, 0.36]", iv);
    }

    #[test]
    fn test_vol_slice_get_iv_at_strike_extrapolate() {
        let spot = 100.0;
        let tte = 30.0 / 365.0;
        let rfr = 0.05;
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();

        let pairs = vec![
            (0.40, 0.30),  // Lower delta (OTM call) → higher strike
            (0.60, 0.25),  // Higher delta (ITM call) → lower strike
        ];
        let slice = VolSlice::from_delta_iv_pairs(pairs, spot, tte, rfr, expiration);

        // Get strike range
        let strike_40d = slice.delta_to_strike(0.40, true).unwrap();  // Higher strike
        let strike_60d = slice.delta_to_strike(0.60, true).unwrap();  // Lower strike

        // Note: For calls, delta 0.60 gives LOWER strike than delta 0.40
        let min_strike = strike_60d.min(strike_40d);
        let max_strike = strike_60d.max(strike_40d);

        // Below range: should use lowest strike's IV (flat extrapolation)
        // Lowest strike is from delta 0.60 → IV 0.25
        let very_low_strike = min_strike - 20.0;
        let iv_low = slice.get_iv_at_strike(very_low_strike);
        assert!(iv_low.is_some());
        assert_relative_eq!(iv_low.unwrap(), 0.25, epsilon = 0.01);

        // Above range: should use highest strike's IV (flat extrapolation)
        // Highest strike is from delta 0.40 → IV 0.30
        let very_high_strike = max_strike + 20.0;
        let iv_high = slice.get_iv_at_strike(very_high_strike);
        assert!(iv_high.is_some());
        assert_relative_eq!(iv_high.unwrap(), 0.30, epsilon = 0.01);
    }

    #[test]
    fn test_vol_slice_get_iv_at_strike_empty() {
        let expiration = NaiveDate::from_ymd_opt(2025, 7, 20).unwrap();
        let slice = VolSlice::from_points(&[], 100.0, 0.1, 0.05, expiration);

        assert!(slice.get_iv_at_strike(100.0).is_none());
    }
}
