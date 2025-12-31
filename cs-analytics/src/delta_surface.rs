// Multi-expiry volatility surface in delta-space

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};

use crate::iv_surface::IVSurface;
use crate::vol_slice::{delta_to_strike_with_iv, VolSlice};

/// Multi-expiry volatility surface in delta-space.
///
/// This structure holds volatility slices for multiple expirations,
/// allowing interpolation across both delta (within each slice) and
/// time (across slices in variance-space).
#[derive(Debug, Clone)]
pub struct DeltaVolSurface {
    /// Slices indexed by expiration
    slices: BTreeMap<NaiveDate, VolSlice>,
    /// Reference spot
    spot: f64,
    /// Surface timestamp
    as_of: DateTime<Utc>,
    /// Symbol
    symbol: String,
    /// Risk-free rate
    risk_free_rate: f64,
}

impl DeltaVolSurface {
    /// Create a new empty surface
    pub fn new(spot: f64, as_of: DateTime<Utc>, symbol: String, risk_free_rate: f64) -> Self {
        Self {
            slices: BTreeMap::new(),
            spot,
            as_of,
            symbol,
            risk_free_rate,
        }
    }

    /// Build from IVSurface (strike-space to delta-space conversion)
    pub fn from_iv_surface(surface: &IVSurface, risk_free_rate: f64) -> Self {
        let spot: f64 = surface.spot_price().try_into().unwrap_or(0.0);
        let as_of = surface.as_of_time();

        // Group points by expiration
        let mut by_expiry: BTreeMap<NaiveDate, Vec<(f64, f64)>> = BTreeMap::new();

        for point in surface.points() {
            let strike: f64 = point.strike.try_into().unwrap_or(0.0);
            if point.iv > 0.0 && strike > 0.0 {
                by_expiry
                    .entry(point.expiration)
                    .or_default()
                    .push((strike, point.iv));
            }
        }

        // Build slices
        let mut slices = BTreeMap::new();
        for (exp, points) in by_expiry {
            let tte = (exp - as_of.date_naive()).num_days() as f64 / 365.0;
            if tte > 0.0 {
                let slice = VolSlice::from_points(&points, spot, tte, risk_free_rate, exp);
                slices.insert(exp, slice);
            }
        }

        Self {
            slices,
            spot,
            as_of,
            symbol: surface.underlying().to_string(),
            risk_free_rate,
        }
    }

    /// Add a slice to the surface
    pub fn add_slice(&mut self, slice: VolSlice) {
        self.slices.insert(slice.expiration(), slice);
    }

    pub fn spot(&self) -> f64 {
        self.spot
    }

    pub fn as_of(&self) -> DateTime<Utc> {
        self.as_of
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn risk_free_rate(&self) -> f64 {
        self.risk_free_rate
    }

    /// Get available expirations
    pub fn expirations(&self) -> Vec<NaiveDate> {
        self.slices.keys().copied().collect()
    }

    /// Get a specific slice by expiration
    pub fn slice(&self, expiration: NaiveDate) -> Option<&VolSlice> {
        self.slices.get(&expiration)
    }

    /// Time to expiry in years for a given date
    pub fn tte(&self, expiration: NaiveDate) -> Option<f64> {
        let days = (expiration - self.as_of.date_naive()).num_days();
        if days > 0 {
            Some(days as f64 / 365.0)
        } else {
            None
        }
    }

    /// Get IV at target delta and expiration.
    ///
    /// Uses linear interpolation within a slice (by delta) and
    /// linear interpolation in variance-time across expiries.
    pub fn get_iv(&self, delta: f64, expiration: NaiveDate) -> Option<f64> {
        // Exact match
        if let Some(slice) = self.slices.get(&expiration) {
            return slice.get_iv(delta);
        }

        // Find bracketing expiries
        let mut lower: Option<(&NaiveDate, &VolSlice)> = None;
        let mut upper: Option<(&NaiveDate, &VolSlice)> = None;

        for (exp, slice) in &self.slices {
            if *exp <= expiration {
                lower = Some((exp, slice));
            } else if upper.is_none() {
                upper = Some((exp, slice));
                break;
            }
        }

        match (lower, upper) {
            (Some((_, s1)), Some((_, s2))) => {
                // Interpolate in variance-time space
                let var1 = s1.get_total_variance(delta)?;
                let var2 = s2.get_total_variance(delta)?;

                let t1 = s1.tte();
                let t2 = s2.tte();
                let t_target = self.tte(expiration)?;

                // Linear interpolation in variance
                let denom = t2 - t1;
                if denom.abs() < 1e-10 {
                    return Some(s1.get_iv(delta)?);
                }
                let var_target = var1 + (var2 - var1) * (t_target - t1) / denom;

                // Back out IV
                if t_target > 0.0 && var_target >= 0.0 {
                    Some((var_target / t_target).sqrt())
                } else {
                    None
                }
            }
            (Some((_, s)), None) => s.get_iv(delta),
            (None, Some((_, s))) => s.get_iv(delta),
            (None, None) => None,
        }
    }

    /// Get total variance at delta and expiration
    pub fn get_total_variance(&self, delta: f64, expiration: NaiveDate) -> Option<f64> {
        let iv = self.get_iv(delta, expiration)?;
        let tte = self.tte(expiration)?;
        Some(iv * iv * tte)
    }

    /// Get term structure at fixed delta
    ///
    /// Returns (expiration, IV) pairs for all slices.
    pub fn term_structure(&self, delta: f64) -> Vec<(NaiveDate, f64)> {
        self.slices
            .iter()
            .filter_map(|(exp, slice)| {
                slice.get_iv(delta).map(|iv| (*exp, iv))
            })
            .collect()
    }

    /// Get smile at fixed expiration
    ///
    /// Returns (delta, IV) pairs for the slice.
    pub fn smile(&self, expiration: NaiveDate) -> Option<Vec<(f64, f64)>> {
        let slice = self.slices.get(&expiration)?;
        Some(slice.smile_points().to_vec())
    }

    /// Map delta to strike at given expiration
    pub fn delta_to_strike(
        &self,
        delta: f64,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Get IV at this delta
        let iv = self.get_iv(delta, expiration)?;
        let tte = self.tte(expiration)?;

        if tte <= 0.0 {
            return None;
        }

        delta_to_strike_with_iv(delta, iv, self.spot, tte, self.risk_free_rate, is_call)
    }

    /// Compute forward variance between two expiries at a given delta
    ///
    /// Forward variance = Var(T2) - Var(T1) which must be >= 0 for no calendar arbitrage.
    pub fn forward_variance(
        &self,
        delta: f64,
        near_expiry: NaiveDate,
        far_expiry: NaiveDate,
    ) -> Option<f64> {
        let var_near = self.get_total_variance(delta, near_expiry)?;
        let var_far = self.get_total_variance(delta, far_expiry)?;
        Some(var_far - var_near)
    }

    /// Check if there's calendar arbitrage at a given delta between two expiries
    ///
    /// Calendar arbitrage exists if forward variance is negative.
    pub fn has_calendar_arbitrage(
        &self,
        delta: f64,
        near_expiry: NaiveDate,
        far_expiry: NaiveDate,
    ) -> bool {
        self.forward_variance(delta, near_expiry, far_expiry)
            .map(|fv| fv < -1e-10)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal::Decimal;

    fn create_test_surface() -> DeltaVolSurface {
        let now = Utc::now();
        let base_date = now.date_naive();
        let spot = 100.0;
        let rfr = 0.05;

        let mut surface = DeltaVolSurface::new(spot, now, "TEST".to_string(), rfr);

        // Add 30-day slice
        let exp1 = base_date + chrono::Duration::days(30);
        let tte1 = 30.0 / 365.0;
        let slice1 = VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.35),
                (0.50, 0.30),
                (0.75, 0.28),
            ],
            spot, tte1, rfr, exp1,
        );
        surface.add_slice(slice1);

        // Add 60-day slice (lower IV - normal term structure)
        let exp2 = base_date + chrono::Duration::days(60);
        let tte2 = 60.0 / 365.0;
        let slice2 = VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.32),
                (0.50, 0.28),
                (0.75, 0.26),
            ],
            spot, tte2, rfr, exp2,
        );
        surface.add_slice(slice2);

        surface
    }

    #[test]
    fn test_delta_surface_from_iv_surface() {
        let now = Utc::now();
        let base_date = now.date_naive();

        let points = vec![
            crate::iv_surface::IVPoint {
                strike: Decimal::new(95, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.30,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250130C95".to_string(),
            },
            crate::iv_surface::IVPoint {
                strike: Decimal::new(100, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.25,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250130C100".to_string(),
            },
        ];

        let iv_surface = IVSurface::new(
            points,
            "TEST".to_string(),
            now,
            Decimal::new(100, 0),
        );

        let delta_surface = DeltaVolSurface::from_iv_surface(&iv_surface, 0.05);

        assert_eq!(delta_surface.symbol(), "TEST");
        assert_eq!(delta_surface.expirations().len(), 1);
    }

    #[test]
    fn test_delta_surface_get_iv_exact() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let exp1 = expirations[0];

        // Get IV at exact expiry and exact delta
        let iv = surface.get_iv(0.50, exp1);
        assert!(iv.is_some());
        assert_relative_eq!(iv.unwrap(), 0.30, epsilon = 1e-6);
    }

    #[test]
    fn test_delta_surface_get_iv_interpolated_time() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let exp1 = expirations[0]; // 30 days
        let exp2 = expirations[1]; // 60 days

        // Get IV at 45 days (between exp1 and exp2)
        let base_date = surface.as_of().date_naive();
        let mid_exp = base_date + chrono::Duration::days(45);

        let iv = surface.get_iv(0.50, mid_exp);
        assert!(iv.is_some());

        // Should be between 30-day IV (0.30) and 60-day IV (0.28)
        let iv = iv.unwrap();
        assert!(iv > 0.28 && iv < 0.30, "IV {} should be between 0.28 and 0.30", iv);
    }

    #[test]
    fn test_delta_surface_term_structure() {
        let surface = create_test_surface();

        let ts = surface.term_structure(0.50);
        assert_eq!(ts.len(), 2);

        // First expiry should have higher IV (30-day)
        assert_relative_eq!(ts[0].1, 0.30, epsilon = 1e-6);
        // Second expiry should have lower IV (60-day)
        assert_relative_eq!(ts[1].1, 0.28, epsilon = 1e-6);
    }

    #[test]
    fn test_delta_surface_smile() {
        let surface = create_test_surface();
        let expirations = surface.expirations();

        let smile = surface.smile(expirations[0]);
        assert!(smile.is_some());
        assert_eq!(smile.unwrap().len(), 3);
    }

    #[test]
    fn test_delta_surface_delta_to_strike() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let exp = expirations[0];

        let strike = surface.delta_to_strike(0.50, exp, true);
        assert!(strike.is_some());

        // 50 delta strike should be near spot
        let strike = strike.unwrap();
        assert!((strike - 100.0).abs() < 10.0,
                "50 delta strike {} should be near spot 100.0", strike);
    }

    #[test]
    fn test_delta_surface_forward_variance() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let exp1 = expirations[0]; // 30 days
        let exp2 = expirations[1]; // 60 days

        let fwd_var = surface.forward_variance(0.50, exp1, exp2);
        assert!(fwd_var.is_some());

        // Forward variance should be positive (no calendar arbitrage in test data)
        assert!(fwd_var.unwrap() >= 0.0, "Forward variance should be non-negative");
    }

    #[test]
    fn test_delta_surface_no_calendar_arbitrage() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let exp1 = expirations[0];
        let exp2 = expirations[1];

        // Normal term structure should not have calendar arbitrage
        assert!(!surface.has_calendar_arbitrage(0.50, exp1, exp2));
    }

    #[test]
    fn test_delta_surface_tte() {
        let surface = create_test_surface();
        let expirations = surface.expirations();

        let tte = surface.tte(expirations[0]);
        assert!(tte.is_some());
        assert_relative_eq!(tte.unwrap(), 30.0 / 365.0, epsilon = 1e-6);
    }
}
