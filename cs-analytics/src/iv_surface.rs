use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

use crate::iv_model::PricingIVProvider;

/// Single IV observation
#[derive(Debug, Clone)]
pub struct IVPoint {
    pub strike: Decimal,
    pub expiration: NaiveDate,
    pub iv: f64,
    pub timestamp: DateTime<Utc>,
    pub underlying_price: Decimal,
    pub is_call: bool,
    pub contract_ticker: String,
}

impl IVPoint {
    pub fn moneyness(&self) -> f64 {
        if self.underlying_price.is_zero() {
            return 1.0;
        }
        (self.strike / self.underlying_price).try_into().unwrap_or(1.0)
    }

    pub fn is_atm(&self, tolerance: f64) -> bool {
        (self.moneyness() - 1.0).abs() <= tolerance
    }
}

/// Implied volatility surface: σ(K, T)
#[derive(Debug, Clone)]
pub struct IVSurface {
    points: Vec<IVPoint>,
    underlying: String,
    as_of_time: DateTime<Utc>,
    spot_price: Decimal,
}

impl IVSurface {
    pub fn new(
        points: Vec<IVPoint>,
        underlying: String,
        as_of_time: DateTime<Utc>,
        spot_price: Decimal,
    ) -> Self {
        Self { points, underlying, as_of_time, spot_price }
    }

    pub fn underlying(&self) -> &str { &self.underlying }
    pub fn as_of_time(&self) -> DateTime<Utc> { self.as_of_time }
    pub fn spot_price(&self) -> Decimal { self.spot_price }
    pub fn points(&self) -> &[IVPoint] { &self.points }

    /// Interpolate IV for given strike/expiration
    pub fn get_iv(
        &self,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        let matching: Vec<_> = self.points.iter()
            .filter(|p| p.is_call == is_call)
            .collect();

        if matching.is_empty() {
            return None;
        }

        // Group by expiration
        let mut by_expiry: BTreeMap<NaiveDate, Vec<&IVPoint>> = BTreeMap::new();
        for p in &matching {
            by_expiry.entry(p.expiration).or_default().push(p);
        }

        // Try exact expiration first
        if let Some(points) = by_expiry.get(&expiration) {
            if let Some(iv) = self.interpolate_strike(points, strike) {
                return Some(iv);
            }
        }

        // Interpolate across expirations
        self.interpolate_expiration(&by_expiry, strike, expiration)
    }

    /// Get IV using a specific interpolation model
    ///
    /// This allows switching between sticky strike, sticky moneyness,
    /// and sticky delta interpolation strategies.
    pub fn get_iv_with_model(
        &self,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
        model: &dyn PricingIVProvider,
    ) -> Option<f64> {
        model.get_iv(self, strike, expiration, is_call)
    }

    /// Get IV at moneyness and TTM
    pub fn get_iv_by_moneyness_ttm(
        &self,
        moneyness: f64,
        ttm_days: i32,
        is_call: bool,
    ) -> Option<f64> {
        let strike_f64: f64 = self.spot_price.try_into().unwrap_or(0.0);
        let strike = Decimal::try_from(strike_f64 * moneyness).ok()?;
        let target_expiry = self.as_of_time.date_naive() + chrono::Duration::days(ttm_days as i64);
        self.get_iv(strike, target_expiry, is_call)
    }

    /// Get ATM term structure
    pub fn get_atm_term_structure(&self, is_call: bool) -> BTreeMap<NaiveDate, f64> {
        let mut result = BTreeMap::new();

        let matching: Vec<_> = self.points.iter()
            .filter(|p| p.is_call == is_call)
            .collect();

        let mut by_expiry: BTreeMap<NaiveDate, Vec<&IVPoint>> = BTreeMap::new();
        for p in &matching {
            by_expiry.entry(p.expiration).or_default().push(p);
        }

        for (exp, points) in by_expiry {
            if let Some(iv) = self.interpolate_strike(&points, self.spot_price) {
                result.insert(exp, iv);
            }
        }

        result
    }

    fn interpolate_strike(&self, points: &[&IVPoint], target_strike: Decimal) -> Option<f64> {
        if points.is_empty() {
            return None;
        }

        let mut sorted: Vec<_> = points.iter().collect();
        sorted.sort_by_key(|p| p.strike);

        // Exact match
        if let Some(p) = sorted.iter().find(|p| p.strike == target_strike) {
            return Some(p.iv);
        }

        // Find bracketing strikes
        let mut lower: Option<&IVPoint> = None;
        let mut upper: Option<&IVPoint> = None;

        for p in sorted {
            if p.strike < target_strike {
                lower = Some(p);
            } else if p.strike > target_strike && upper.is_none() {
                upper = Some(p);
                break;
            }
        }

        match (lower, upper) {
            (Some(l), Some(u)) => {
                let range: f64 = (u.strike - l.strike).try_into().unwrap_or(1.0);
                if range == 0.0 { return Some(l.iv); }
                let weight: f64 = ((target_strike - l.strike) / (u.strike - l.strike))
                    .try_into().unwrap_or(0.5);
                Some(l.iv + weight * (u.iv - l.iv))
            }
            (Some(l), None) => Some(l.iv),
            (None, Some(u)) => Some(u.iv),
            (None, None) => None,
        }
    }

    fn interpolate_expiration(
        &self,
        by_expiry: &BTreeMap<NaiveDate, Vec<&IVPoint>>,
        target_strike: Decimal,
        target_expiration: NaiveDate,
    ) -> Option<f64> {
        // Get IV at target strike for each expiration
        let mut expiry_ivs: Vec<(NaiveDate, f64)> = Vec::new();
        for (exp, points) in by_expiry {
            if let Some(iv) = self.interpolate_strike(points, target_strike) {
                expiry_ivs.push((*exp, iv));
            }
        }

        if expiry_ivs.is_empty() {
            return None;
        }

        expiry_ivs.sort_by_key(|(exp, _)| *exp);

        // Find bracketing expirations
        let mut lower: Option<(NaiveDate, f64)> = None;
        let mut upper: Option<(NaiveDate, f64)> = None;

        for (exp, iv) in &expiry_ivs {
            if *exp < target_expiration {
                lower = Some((*exp, *iv));
            } else if *exp > target_expiration && upper.is_none() {
                upper = Some((*exp, *iv));
                break;
            } else if *exp == target_expiration {
                return Some(*iv);
            }
        }

        match (lower, upper) {
            (Some((l_exp, l_iv)), Some((u_exp, u_iv))) => {
                // sqrt(time) weighted interpolation
                let as_of = self.as_of_time.date_naive();
                let sqrt_time = |exp: NaiveDate| -> f64 {
                    ((exp - as_of).num_days().max(1) as f64 / 365.0).sqrt()
                };

                let sqrt_lower = sqrt_time(l_exp);
                let sqrt_upper = sqrt_time(u_exp);
                let sqrt_target = sqrt_time(target_expiration);

                let range = sqrt_upper - sqrt_lower;
                if range == 0.0 { return Some(l_iv); }

                let weight = (sqrt_target - sqrt_lower) / range;
                Some(l_iv + weight * (u_iv - l_iv))
            }
            (Some((_, iv)), None) => Some(iv),
            (None, Some((_, iv))) => Some(iv),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_surface() -> IVSurface {
        let now = Utc::now();
        let base_date = now.date_naive();

        let points = vec![
            IVPoint {
                strike: Decimal::new(95, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.25,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250130C95".to_string(),
            },
            IVPoint {
                strike: Decimal::new(100, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.30,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250130C100".to_string(),
            },
            IVPoint {
                strike: Decimal::new(105, 0),
                expiration: base_date + chrono::Duration::days(30),
                iv: 0.28,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250130C105".to_string(),
            },
            IVPoint {
                strike: Decimal::new(100, 0),
                expiration: base_date + chrono::Duration::days(60),
                iv: 0.32,
                timestamp: now,
                underlying_price: Decimal::new(100, 0),
                is_call: true,
                contract_ticker: "TEST250228C100".to_string(),
            },
        ];

        IVSurface::new(points, "TEST".to_string(), now, Decimal::new(100, 0))
    }

    #[test]
    fn test_iv_point_moneyness() {
        let point = IVPoint {
            strike: Decimal::new(105, 0),
            expiration: NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            iv: 0.30,
            timestamp: Utc::now(),
            underlying_price: Decimal::new(100, 0),
            is_call: true,
            contract_ticker: "TEST250620C105".to_string(),
        };

        assert_eq!(point.moneyness(), 1.05);
    }

    #[test]
    fn test_iv_point_is_atm() {
        let point = IVPoint {
            strike: Decimal::new(100, 0),
            expiration: NaiveDate::from_ymd_opt(2025, 6, 20).unwrap(),
            iv: 0.30,
            timestamp: Utc::now(),
            underlying_price: Decimal::new(100, 0),
            is_call: true,
            contract_ticker: "TEST250620C100".to_string(),
        };

        assert!(point.is_atm(0.01));
    }

    #[test]
    fn test_iv_surface_exact_match() {
        let surface = create_test_surface();
        let base_date = surface.as_of_time().date_naive();

        let iv = surface.get_iv(
            Decimal::new(100, 0),
            base_date + chrono::Duration::days(30),
            true
        );

        assert!(iv.is_some());
        assert_eq!(iv.unwrap(), 0.30);
    }

    #[test]
    fn test_iv_surface_strike_interpolation() {
        let surface = create_test_surface();
        let base_date = surface.as_of_time().date_naive();

        // Interpolate between 95 (IV=0.25) and 100 (IV=0.30)
        let iv = surface.get_iv(
            Decimal::new(975, 1), // 97.5
            base_date + chrono::Duration::days(30),
            true
        );

        assert!(iv.is_some());
        let interpolated = iv.unwrap();
        // Should be halfway between 0.25 and 0.30 = 0.275
        assert!((interpolated - 0.275).abs() < 0.001);
    }

    #[test]
    fn test_iv_surface_expiration_interpolation() {
        let surface = create_test_surface();
        let base_date = surface.as_of_time().date_naive();

        // Interpolate between 30 days (IV=0.30) and 60 days (IV=0.32) for strike 100
        let iv = surface.get_iv(
            Decimal::new(100, 0),
            base_date + chrono::Duration::days(45),
            true
        );

        assert!(iv.is_some());
        let interpolated = iv.unwrap();
        // Should be between 0.30 and 0.32
        assert!(interpolated > 0.30 && interpolated < 0.32);
    }

    #[test]
    fn test_iv_surface_atm_term_structure() {
        let surface = create_test_surface();

        let term_structure = surface.get_atm_term_structure(true);

        assert_eq!(term_structure.len(), 2); // Two expirations
    }

    #[test]
    fn test_iv_surface_empty_points() {
        let now = Utc::now();
        let surface = IVSurface::new(
            vec![],
            "TEST".to_string(),
            now,
            Decimal::new(100, 0)
        );

        let iv = surface.get_iv(
            Decimal::new(100, 0),
            now.date_naive() + chrono::Duration::days(30),
            true
        );

        assert!(iv.is_none());
    }

    #[test]
    fn test_iv_surface_extrapolation_lower() {
        let surface = create_test_surface();
        let base_date = surface.as_of_time().date_naive();

        // Strike below all available strikes (should use lowest)
        let iv = surface.get_iv(
            Decimal::new(90, 0),
            base_date + chrono::Duration::days(30),
            true
        );

        assert!(iv.is_some());
        assert_eq!(iv.unwrap(), 0.25); // Should use 95 strike value
    }

    #[test]
    fn test_iv_surface_extrapolation_upper() {
        let surface = create_test_surface();
        let base_date = surface.as_of_time().date_naive();

        // Strike above all available strikes (should use highest)
        let iv = surface.get_iv(
            Decimal::new(110, 0),
            base_date + chrono::Duration::days(30),
            true
        );

        assert!(iv.is_some());
        assert_eq!(iv.unwrap(), 0.28); // Should use 105 strike value
    }
}
