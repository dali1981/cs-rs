//! Pricing IV Interpolation Models
//!
//! Implements different volatility surface interpolation strategies for option pricing:
//! - Sticky Strike: IV indexed by absolute strike K
//! - Sticky Moneyness: IV indexed by K/S
//! - Sticky Delta: IV indexed by Black-Scholes delta Δ
//!
//! Reference: Derman (1999) "Regimes of Volatility"

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

use crate::black_scholes::bs_delta;
use crate::iv_surface::{IVPoint, IVSurface};

/// Provider for IV interpolation during option pricing
///
/// This trait defines how to interpolate implied volatility from market
/// observations when pricing options at strikes/expirations without direct quotes.
pub trait PricingIVProvider: Send + Sync {
    /// Get IV for a specific option
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64>;

    /// Model name for logging/debugging
    fn name(&self) -> &'static str;
}

// =============================================================================
// Sticky Strike Pricing (Current Default)
// =============================================================================

/// Sticky Strike: IV indexed by absolute strike K
///
/// When spot moves, the smile stays anchored to the same strike values.
/// This is the simplest model and matches the current implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct StickyStrikePricing;

impl PricingIVProvider for StickyStrikePricing {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Use the existing IVSurface implementation
        surface.get_iv(strike, expiration, is_call)
    }

    fn name(&self) -> &'static str {
        "sticky_strike"
    }
}

// =============================================================================
// Sticky Moneyness Pricing
// =============================================================================

/// Sticky Moneyness: IV indexed by K/S (moneyness)
///
/// When spot moves, the smile "floats" with it.
/// Same moneyness → same IV, regardless of absolute strike.
#[derive(Debug, Clone, Copy, Default)]
pub struct StickyMoneynessPricing;

impl PricingIVProvider for StickyMoneynessPricing {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        let spot = surface.spot_price();
        if spot.is_zero() {
            return None;
        }

        let target_moneyness: f64 = (strike / spot).try_into().ok()?;

        interpolate_by_moneyness(surface, target_moneyness, expiration, is_call)
    }

    fn name(&self) -> &'static str {
        "sticky_moneyness"
    }
}

/// Interpolate IV in moneyness (K/S) space
fn interpolate_by_moneyness(
    surface: &IVSurface,
    target_moneyness: f64,
    expiration: NaiveDate,
    is_call: bool,
) -> Option<f64> {
    let matching: Vec<_> = surface
        .points()
        .iter()
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
        if let Some(iv) = interpolate_moneyness_at_expiry(points, target_moneyness) {
            return Some(iv);
        }
    }

    // Interpolate across expirations using sqrt(T) weighting
    interpolate_expiration_by_moneyness(surface, &by_expiry, target_moneyness, expiration)
}

fn interpolate_moneyness_at_expiry(points: &[&IVPoint], target_moneyness: f64) -> Option<f64> {
    if points.is_empty() {
        return None;
    }

    // Convert each point to (moneyness, iv)
    let mut moneyness_iv: Vec<(f64, f64)> = points
        .iter()
        .map(|p| (p.moneyness(), p.iv))
        .collect();

    moneyness_iv.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Find bracketing moneyness values
    let mut lower: Option<(f64, f64)> = None;
    let mut upper: Option<(f64, f64)> = None;

    for (m, iv) in &moneyness_iv {
        if (*m - target_moneyness).abs() < 1e-9 {
            return Some(*iv);
        }
        if *m < target_moneyness {
            lower = Some((*m, *iv));
        } else if upper.is_none() {
            upper = Some((*m, *iv));
            break;
        }
    }

    match (lower, upper) {
        (Some((l_m, l_iv)), Some((u_m, u_iv))) => {
            let range = u_m - l_m;
            if range.abs() < 1e-9 {
                return Some(l_iv);
            }
            let weight = (target_moneyness - l_m) / range;
            Some(l_iv + weight * (u_iv - l_iv))
        }
        (Some((_, iv)), None) => Some(iv),
        (None, Some((_, iv))) => Some(iv),
        (None, None) => None,
    }
}

fn interpolate_expiration_by_moneyness(
    surface: &IVSurface,
    by_expiry: &BTreeMap<NaiveDate, Vec<&IVPoint>>,
    target_moneyness: f64,
    target_expiration: NaiveDate,
) -> Option<f64> {
    // Get IV at target moneyness for each expiration
    let mut expiry_ivs: Vec<(NaiveDate, f64)> = Vec::new();
    for (exp, points) in by_expiry {
        if let Some(iv) = interpolate_moneyness_at_expiry(points, target_moneyness) {
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
        if *exp == target_expiration {
            return Some(*iv);
        }
        if *exp < target_expiration {
            lower = Some((*exp, *iv));
        } else if upper.is_none() {
            upper = Some((*exp, *iv));
            break;
        }
    }

    match (lower, upper) {
        (Some((l_exp, l_iv)), Some((u_exp, u_iv))) => {
            // sqrt(time) weighted interpolation
            let as_of = surface.as_of_time().date_naive();
            let sqrt_time = |exp: NaiveDate| -> f64 {
                ((exp - as_of).num_days().max(1) as f64 / 365.0).sqrt()
            };

            let sqrt_lower = sqrt_time(l_exp);
            let sqrt_upper = sqrt_time(u_exp);
            let sqrt_target = sqrt_time(target_expiration);

            let range = sqrt_upper - sqrt_lower;
            if range.abs() < 1e-9 {
                return Some(l_iv);
            }

            let weight = (sqrt_target - sqrt_lower) / range;
            Some(l_iv + weight * (u_iv - l_iv))
        }
        (Some((_, iv)), None) => Some(iv),
        (None, Some((_, iv))) => Some(iv),
        (None, None) => None,
    }
}

// =============================================================================
// Sticky Delta Pricing
// =============================================================================

/// Sticky Delta: IV indexed by Black-Scholes delta Δ
///
/// When spot moves, the smile floats such that same delta → same IV.
/// This requires iterative solving since delta depends on IV.
///
/// Reference: Derman (1999) "Regimes of Volatility"
#[derive(Debug, Clone)]
pub struct StickyDeltaPricing {
    pub risk_free_rate: f64,
    pub max_iterations: usize,
    pub tolerance: f64,
}

impl Default for StickyDeltaPricing {
    fn default() -> Self {
        Self {
            risk_free_rate: 0.05,
            max_iterations: 50,
            tolerance: 1e-6,
        }
    }
}

impl StickyDeltaPricing {
    pub fn new(risk_free_rate: f64) -> Self {
        Self {
            risk_free_rate,
            ..Default::default()
        }
    }

    /// Build a delta smile from the IV surface
    fn build_delta_smile(&self, surface: &IVSurface) -> DeltaSmile {
        let spot: f64 = surface.spot_price().try_into().unwrap_or(0.0);
        let as_of = surface.as_of_time();

        let points: Vec<DeltaIVPoint> = surface
            .points()
            .iter()
            .filter_map(|p| {
                let strike: f64 = p.strike.try_into().ok()?;
                let ttm = (p.expiration - as_of.date_naive()).num_days() as f64 / 365.0;
                if ttm <= 0.0 || spot <= 0.0 {
                    return None;
                }

                // Compute delta using the point's own IV
                let delta = bs_delta(spot, strike, ttm, p.iv, p.is_call, self.risk_free_rate);

                Some(DeltaIVPoint {
                    delta,
                    iv: p.iv,
                    expiration: p.expiration,
                    is_call: p.is_call,
                })
            })
            .collect();

        DeltaSmile { points }
    }
}

impl PricingIVProvider for StickyDeltaPricing {
    fn get_iv(
        &self,
        surface: &IVSurface,
        strike: Decimal,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        let spot: f64 = surface.spot_price().try_into().ok()?;
        let strike_f64: f64 = strike.try_into().ok()?;
        let ttm = (expiration - surface.as_of_time().date_naive()).num_days() as f64 / 365.0;

        if ttm <= 0.0 || spot <= 0.0 {
            return None;
        }

        // Build delta smile from surface
        let delta_smile = self.build_delta_smile(surface);

        // Get ATM vol as initial guess - required for iteration
        // Return None if no ATM vol available (insufficient surface data)
        let mut sigma = delta_smile
            .get_atm_iv(expiration, is_call)?;

        // Iterative solve: find σ such that σ = smile(Δ(K, σ))
        for _ in 0..self.max_iterations {
            // Compute delta at current sigma
            let delta = bs_delta(spot, strike_f64, ttm, sigma, is_call, self.risk_free_rate);

            // Look up IV for this delta
            let new_sigma = match delta_smile.interpolate_by_delta(delta, expiration, is_call) {
                Some(s) => s,
                None => return Some(sigma), // Can't interpolate, return current estimate
            };

            // Check convergence
            if (new_sigma - sigma).abs() < self.tolerance {
                return Some(new_sigma);
            }

            sigma = new_sigma;
        }

        Some(sigma) // Return best estimate even if not fully converged
    }

    fn name(&self) -> &'static str {
        "sticky_delta"
    }
}

// =============================================================================
// Delta Smile Helper
// =============================================================================

/// A point on the delta-parameterized smile
#[derive(Debug, Clone)]
struct DeltaIVPoint {
    delta: f64,
    iv: f64,
    expiration: NaiveDate,
    is_call: bool,
}

/// Smile parameterized by delta
struct DeltaSmile {
    points: Vec<DeltaIVPoint>,
}

impl DeltaSmile {
    /// Interpolate IV for a given delta value
    fn interpolate_by_delta(
        &self,
        target_delta: f64,
        expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Filter to matching expiration and option type
        let mut matching: Vec<_> = self
            .points
            .iter()
            .filter(|p| p.expiration == expiration && p.is_call == is_call)
            .collect();

        if matching.is_empty() {
            // Fall back to nearest expiration
            return self.interpolate_nearest_expiry(target_delta, expiration, is_call);
        }

        // Sort by delta
        matching.sort_by(|a, b| a.delta.partial_cmp(&b.delta).unwrap_or(std::cmp::Ordering::Equal));

        // Find bracketing deltas
        let mut lower: Option<&DeltaIVPoint> = None;
        let mut upper: Option<&DeltaIVPoint> = None;

        for p in &matching {
            if (p.delta - target_delta).abs() < 1e-9 {
                return Some(p.iv);
            }
            if p.delta < target_delta {
                lower = Some(p);
            } else if upper.is_none() {
                upper = Some(p);
                break;
            }
        }

        match (lower, upper) {
            (Some(l), Some(u)) => {
                let range = u.delta - l.delta;
                if range.abs() < 1e-9 {
                    return Some(l.iv);
                }
                let weight = (target_delta - l.delta) / range;
                Some(l.iv + weight * (u.iv - l.iv))
            }
            (Some(l), None) => Some(l.iv),
            (None, Some(u)) => Some(u.iv),
            (None, None) => None,
        }
    }

    /// Fall back to nearest available expiration
    fn interpolate_nearest_expiry(
        &self,
        target_delta: f64,
        target_expiration: NaiveDate,
        is_call: bool,
    ) -> Option<f64> {
        // Find the closest expiration that has data
        let available_expiries: Vec<_> = self
            .points
            .iter()
            .filter(|p| p.is_call == is_call)
            .map(|p| p.expiration)
            .collect();

        let nearest = available_expiries
            .iter()
            .min_by_key(|exp| ((**exp - target_expiration).num_days()).abs())?;

        self.interpolate_by_delta(target_delta, *nearest, is_call)
    }

    /// Get ATM IV (delta ≈ 0.5 for calls, -0.5 for puts)
    fn get_atm_iv(&self, expiration: NaiveDate, is_call: bool) -> Option<f64> {
        let atm_delta = if is_call { 0.5 } else { -0.5 };
        self.interpolate_by_delta(atm_delta, expiration, is_call)
    }
}

// =============================================================================
// PricingModel Enum (for configuration)
// =============================================================================

/// Pricing IV interpolation model selection
///
/// Determines how to interpolate implied volatility when pricing options
/// at strikes/expirations without direct market quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingModel {
    #[default]
    StickyStrike,
    StickyMoneyness,
    StickyDelta,
}

impl PricingModel {
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().replace('-', "_").as_str() {
            "sticky_moneyness" | "moneyness" => PricingModel::StickyMoneyness,
            "sticky_delta" | "delta" => PricingModel::StickyDelta,
            _ => PricingModel::StickyStrike,
        }
    }

    /// Create the corresponding pricing provider
    pub fn to_provider(&self) -> Box<dyn PricingIVProvider> {
        match self {
            PricingModel::StickyStrike => Box::new(StickyStrikePricing),
            PricingModel::StickyMoneyness => Box::new(StickyMoneynessPricing),
            PricingModel::StickyDelta => Box::new(StickyDeltaPricing::default()),
        }
    }

    /// Create pricing provider with custom risk-free rate (for sticky delta)
    pub fn to_provider_with_rate(&self, risk_free_rate: f64) -> Box<dyn PricingIVProvider> {
        match self {
            PricingModel::StickyStrike => Box::new(StickyStrikePricing),
            PricingModel::StickyMoneyness => Box::new(StickyMoneynessPricing),
            PricingModel::StickyDelta => Box::new(StickyDeltaPricing::new(risk_free_rate)),
        }
    }
}

impl std::fmt::Display for PricingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PricingModel::StickyStrike => write!(f, "sticky_strike"),
            PricingModel::StickyMoneyness => write!(f, "sticky_moneyness"),
            PricingModel::StickyDelta => write!(f, "sticky_delta"),
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn dec(val: i64) -> Decimal {
        Decimal::new(val, 0)
    }

    fn create_test_surface(spot: f64) -> IVSurface {
        let now = Utc::now();
        let base_date = now.date_naive();
        let exp_30d = base_date + chrono::Duration::days(30);
        let spot_dec = Decimal::try_from(spot).unwrap();

        // Create a smile with skew: OTM puts have higher IV
        let points = vec![
            // 30-day expiration
            IVPoint {
                strike: Decimal::try_from(spot * 0.90).unwrap(), // OTM put
                expiration: exp_30d,
                iv: 0.40,
                timestamp: now,
                underlying_price: spot_dec,
                is_call: false,
                contract_ticker: format!("TEST{}P{}", exp_30d.format("%y%m%d"), (spot * 0.90) as i64),
            },
            IVPoint {
                strike: Decimal::try_from(spot * 0.95).unwrap(), // Slightly OTM put
                expiration: exp_30d,
                iv: 0.35,
                timestamp: now,
                underlying_price: spot_dec,
                is_call: false,
                contract_ticker: format!("TEST{}P{}", exp_30d.format("%y%m%d"), (spot * 0.95) as i64),
            },
            IVPoint {
                strike: spot_dec, // ATM
                expiration: exp_30d,
                iv: 0.30,
                timestamp: now,
                underlying_price: spot_dec,
                is_call: true,
                contract_ticker: format!("TEST{}C{}", exp_30d.format("%y%m%d"), spot as i64),
            },
            IVPoint {
                strike: spot_dec, // ATM put
                expiration: exp_30d,
                iv: 0.30,
                timestamp: now,
                underlying_price: spot_dec,
                is_call: false,
                contract_ticker: format!("TEST{}P{}", exp_30d.format("%y%m%d"), spot as i64),
            },
            IVPoint {
                strike: Decimal::try_from(spot * 1.05).unwrap(), // Slightly OTM call
                expiration: exp_30d,
                iv: 0.28,
                timestamp: now,
                underlying_price: spot_dec,
                is_call: true,
                contract_ticker: format!("TEST{}C{}", exp_30d.format("%y%m%d"), (spot * 1.05) as i64),
            },
            IVPoint {
                strike: Decimal::try_from(spot * 1.10).unwrap(), // OTM call
                expiration: exp_30d,
                iv: 0.26,
                timestamp: now,
                underlying_price: spot_dec,
                is_call: true,
                contract_ticker: format!("TEST{}C{}", exp_30d.format("%y%m%d"), (spot * 1.10) as i64),
            },
        ];

        IVSurface::new(points, "TEST".to_string(), now, spot_dec)
    }

    #[test]
    fn test_sticky_strike_basic() {
        let surface = create_test_surface(100.0);
        let provider = StickyStrikePricing;
        let exp = surface.as_of_time().date_naive() + chrono::Duration::days(30);

        // ATM should give 0.30
        let iv = provider.get_iv(&surface, dec(100), exp, true);
        assert!(iv.is_some());
        assert!((iv.unwrap() - 0.30).abs() < 0.01);
    }

    #[test]
    fn test_sticky_moneyness_basic() {
        let surface = create_test_surface(100.0);
        let provider = StickyMoneynessPricing;
        let exp = surface.as_of_time().date_naive() + chrono::Duration::days(30);

        // Moneyness 1.0 (ATM) should give ~0.30
        let iv = provider.get_iv(&surface, dec(100), exp, true);
        assert!(iv.is_some());
        assert!((iv.unwrap() - 0.30).abs() < 0.01);
    }

    #[test]
    fn test_sticky_moneyness_same_moneyness_different_spot() {
        // Surface 1: spot = 100
        let surface1 = create_test_surface(100.0);
        // Surface 2: spot = 110 (spot moved up)
        let surface2 = create_test_surface(110.0);

        let provider = StickyMoneynessPricing;
        let exp1 = surface1.as_of_time().date_naive() + chrono::Duration::days(30);
        let exp2 = surface2.as_of_time().date_naive() + chrono::Duration::days(30);

        // Moneyness 0.95 in both cases
        // Surface1: K=95, S=100, K/S=0.95
        // Surface2: K=104.5, S=110, K/S=0.95
        let iv1 = provider.get_iv(&surface1, dec(95), exp1, false);
        let iv2 = provider.get_iv(&surface2, Decimal::try_from(104.5).unwrap(), exp2, false);

        assert!(iv1.is_some());
        assert!(iv2.is_some());
        // Same moneyness should give approximately same IV
        assert!((iv1.unwrap() - iv2.unwrap()).abs() < 0.02);
    }

    #[test]
    fn test_sticky_delta_basic() {
        let surface = create_test_surface(100.0);
        let provider = StickyDeltaPricing::default();
        let exp = surface.as_of_time().date_naive() + chrono::Duration::days(30);

        // ATM call should give ~0.30
        let iv = provider.get_iv(&surface, dec(100), exp, true);
        assert!(iv.is_some());
        assert!((iv.unwrap() - 0.30).abs() < 0.05);
    }

    #[test]
    fn test_sticky_delta_convergence() {
        let surface = create_test_surface(100.0);
        let provider = StickyDeltaPricing {
            risk_free_rate: 0.05,
            max_iterations: 100,
            tolerance: 1e-8,
        };
        let exp = surface.as_of_time().date_naive() + chrono::Duration::days(30);

        // Test OTM put
        let iv = provider.get_iv(&surface, dec(95), exp, false);
        assert!(iv.is_some());
        let iv_val = iv.unwrap();
        assert!(iv_val > 0.0 && iv_val < 1.0);
    }

    #[test]
    fn test_pricing_model_from_string() {
        assert_eq!(PricingModel::from_string("sticky_strike"), PricingModel::StickyStrike);
        assert_eq!(PricingModel::from_string("sticky-strike"), PricingModel::StickyStrike);
        assert_eq!(PricingModel::from_string("sticky_moneyness"), PricingModel::StickyMoneyness);
        assert_eq!(PricingModel::from_string("moneyness"), PricingModel::StickyMoneyness);
        assert_eq!(PricingModel::from_string("sticky_delta"), PricingModel::StickyDelta);
        assert_eq!(PricingModel::from_string("delta"), PricingModel::StickyDelta);
        assert_eq!(PricingModel::from_string("unknown"), PricingModel::StickyStrike);
    }

    #[test]
    fn test_pricing_model_to_provider() {
        let model = PricingModel::StickyStrike;
        let provider = model.to_provider();
        assert_eq!(provider.name(), "sticky_strike");

        let model = PricingModel::StickyMoneyness;
        let provider = model.to_provider();
        assert_eq!(provider.name(), "sticky_moneyness");

        let model = PricingModel::StickyDelta;
        let provider = model.to_provider();
        assert_eq!(provider.name(), "sticky_delta");
    }
}
