//! IV comparison models for trade selection
//!
//! This module provides different strategies for comparing implied volatilities
//! when scoring calendar spread opportunities.

use chrono::NaiveDate;

use crate::delta_surface::DeltaVolSurface;

/// Model for trade selection IV comparison
///
/// Determines how to compare IVs between short and long expirations
/// when scoring calendar spread opportunities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionModel {
    /// Compare IVs at same strike (correct for calendar spreads)
    ///
    /// Maps target delta to strike using short expiration, then gets
    /// IVs at that same strike for both expirations. This is the correct
    /// approach since calendar spreads trade at the same strike.
    #[default]
    StrikeSpace,

    /// Compare IVs at same delta (current behavior, incorrect)
    ///
    /// Gets IVs at the target delta for both expirations. This means
    /// different strikes due to forward drift, which doesn't match
    /// what the actual trade does.
    DeltaSpace,
}

impl SelectionModel {
    /// Create a provider instance for this model
    pub fn to_provider(&self) -> Box<dyn SelectionIVProvider> {
        match self {
            Self::StrikeSpace => Box::new(StrikeSpaceSelection),
            Self::DeltaSpace => Box::new(DeltaSpaceSelection),
        }
    }

    /// Parse from string (for CLI)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "strike-space" | "strike" => Some(Self::StrikeSpace),
            "delta-space" | "delta" => Some(Self::DeltaSpace),
            _ => None,
        }
    }
}

/// IV pair result for selection scoring
///
/// Contains the IVs and strike where the comparison was made.
#[derive(Debug, Clone, Copy)]
pub struct SelectionIVPair {
    /// IV for short expiration leg
    pub short_iv: f64,
    /// IV for long expiration leg
    pub long_iv: f64,
    /// Strike where comparison is made
    pub strike: f64,
}

/// Provider for IV comparison during trade selection
///
/// Different implementations provide different strategies for comparing
/// IVs between short and long expirations.
pub trait SelectionIVProvider: Send + Sync {
    /// Get IV pair at a target delta for two expirations
    ///
    /// # Arguments
    /// * `surface` - Multi-expiry volatility surface in delta-space
    /// * `delta` - Target delta (0.0 to 1.0 for calls, -1.0 to 0.0 for puts)
    /// * `short_exp` - Near expiration
    /// * `long_exp` - Far expiration
    /// * `is_call` - Whether this is a call (true) or put (false) spread
    ///
    /// # Returns
    /// `SelectionIVPair` with IVs and the strike where comparison was made,
    /// or `None` if data is unavailable.
    fn get_iv_pair(
        &self,
        surface: &DeltaVolSurface,
        delta: f64,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        is_call: bool,
    ) -> Option<SelectionIVPair>;
}

/// Strike-space selection (correct for calendar spreads)
///
/// Maps delta to strike using the short expiration, then compares
/// IVs at that same strike for both expirations.
#[derive(Debug, Clone, Copy)]
pub struct StrikeSpaceSelection;

impl SelectionIVProvider for StrikeSpaceSelection {
    fn get_iv_pair(
        &self,
        surface: &DeltaVolSurface,
        delta: f64,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        is_call: bool,
    ) -> Option<SelectionIVPair> {
        // 1. Map delta to strike using SHORT expiration
        let strike = surface.delta_to_strike(delta, short_exp, is_call)?;

        // 2. Get slices for both expirations
        let short_slice = surface.slice(short_exp)?;
        let long_slice = surface.slice(long_exp)?;

        // 3. Get IVs at that SAME strike for both expirations
        let short_iv = short_slice.get_iv_at_strike(strike)?;
        let long_iv = long_slice.get_iv_at_strike(strike)?;

        Some(SelectionIVPair {
            short_iv,
            long_iv,
            strike,
        })
    }
}

/// Delta-space selection (current behavior, incorrect)
///
/// Compares IVs at the same delta, which means different strikes
/// for different expirations. This is INCORRECT for calendar spreads
/// but preserved for comparison.
#[derive(Debug, Clone, Copy)]
pub struct DeltaSpaceSelection;

impl SelectionIVProvider for DeltaSpaceSelection {
    fn get_iv_pair(
        &self,
        surface: &DeltaVolSurface,
        delta: f64,
        short_exp: NaiveDate,
        long_exp: NaiveDate,
        is_call: bool,
    ) -> Option<SelectionIVPair> {
        // Get IVs at same delta (different strikes)
        let short_iv = surface.get_iv(delta, short_exp)?;
        let long_iv = surface.get_iv(delta, long_exp)?;

        // Strike is for short expiration only (long leg has different strike)
        let strike = surface.delta_to_strike(delta, short_exp, is_call)?;

        Some(SelectionIVPair {
            short_iv,
            long_iv,
            strike,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delta_surface::DeltaVolSurface;
    use crate::vol_slice::VolSlice;
    use chrono::Utc;

    #[test]
    fn test_selection_model_parse() {
        assert_eq!(SelectionModel::from_str("strike-space"), Some(SelectionModel::StrikeSpace));
        assert_eq!(SelectionModel::from_str("strike"), Some(SelectionModel::StrikeSpace));
        assert_eq!(SelectionModel::from_str("delta-space"), Some(SelectionModel::DeltaSpace));
        assert_eq!(SelectionModel::from_str("delta"), Some(SelectionModel::DeltaSpace));
        assert_eq!(SelectionModel::from_str("invalid"), None);
    }

    #[test]
    fn test_selection_model_default() {
        assert_eq!(SelectionModel::default(), SelectionModel::StrikeSpace);
    }

    #[test]
    fn test_selection_model_to_provider() {
        // Just verify we can create providers without panicking
        let _provider = SelectionModel::StrikeSpace.to_provider();
        let _provider = SelectionModel::DeltaSpace.to_provider();
    }

    fn create_test_surface() -> DeltaVolSurface {
        let now = Utc::now();
        let base_date = now.date_naive();
        let spot = 100.0;
        let rfr = 0.05;

        let mut surface = DeltaVolSurface::new(spot, now, "TEST".to_string(), rfr);

        // Add 7-day slice with higher IV
        let exp_short = base_date + chrono::Duration::days(7);
        let tte_short = 7.0 / 365.0;
        let slice_short = VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.40),
                (0.50, 0.35),
                (0.75, 0.38),
            ],
            spot,
            tte_short,
            rfr,
            exp_short,
        );
        surface.add_slice(slice_short);

        // Add 30-day slice with lower IV
        let exp_long = base_date + chrono::Duration::days(30);
        let tte_long = 30.0 / 365.0;
        let slice_long = VolSlice::from_delta_iv_pairs(
            vec![
                (0.25, 0.32),
                (0.50, 0.28),
                (0.75, 0.30),
            ],
            spot,
            tte_long,
            rfr,
            exp_long,
        );
        surface.add_slice(slice_long);

        surface
    }

    #[test]
    fn test_strike_space_selection_vs_delta_space() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let short_exp = expirations[0];
        let long_exp = expirations[1];

        let strike_provider = StrikeSpaceSelection;
        let delta_provider = DeltaSpaceSelection;

        // Get IV pairs at 50 delta
        let strike_pair = strike_provider
            .get_iv_pair(&surface, 0.50, short_exp, long_exp, true)
            .expect("StrikeSpace should return result");

        let delta_pair = delta_provider
            .get_iv_pair(&surface, 0.50, short_exp, long_exp, true)
            .expect("DeltaSpace should return result");

        // Both should have short IV at 50 delta ≈ 0.35
        assert!((strike_pair.short_iv - 0.35).abs() < 0.01);
        assert!((delta_pair.short_iv - 0.35).abs() < 0.01);

        // DeltaSpace: long IV at 50 delta ≈ 0.28
        assert!((delta_pair.long_iv - 0.28).abs() < 0.01);

        // StrikeSpace: long IV at SAME STRIKE as short 50 delta
        // Due to forward drift, same strike at longer expiry has lower delta
        // So long IV should be BETWEEN the 50 delta IV (0.28) and the lower delta IV
        // This demonstrates the key difference!
        println!("StrikeSpace long IV: {}", strike_pair.long_iv);
        println!("DeltaSpace long IV: {}", delta_pair.long_iv);

        // The IVs should be different (this is the core bug we're fixing)
        assert!(
            (strike_pair.long_iv - delta_pair.long_iv).abs() > 0.001,
            "StrikeSpace and DeltaSpace should give different long IVs"
        );
    }

    #[test]
    fn test_strike_space_selection_same_strike() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let short_exp = expirations[0];
        let long_exp = expirations[1];

        let provider = StrikeSpaceSelection;

        let pair = provider
            .get_iv_pair(&surface, 0.50, short_exp, long_exp, true)
            .expect("Should return result");

        // Verify that both legs use the same strike
        // We can't directly verify the strike values are identical,
        // but we can verify the strike field is populated
        assert!(pair.strike > 0.0);
    }

    #[test]
    fn test_delta_space_selection_same_delta() {
        let surface = create_test_surface();
        let expirations = surface.expirations();
        let short_exp = expirations[0];
        let long_exp = expirations[1];

        let provider = DeltaSpaceSelection;

        let pair = provider
            .get_iv_pair(&surface, 0.50, short_exp, long_exp, true)
            .expect("Should return result");

        // Both IVs should be at 50 delta
        // Short: 50 delta @ 7 DTE
        // Long:  50 delta @ 30 DTE (different strike!)
        assert!((pair.short_iv - 0.35).abs() < 0.01);
        assert!((pair.long_iv - 0.28).abs() < 0.01);
    }

    #[test]
    fn test_selection_providers_return_none_on_empty_surface() {
        let now = Utc::now();
        let empty_surface = DeltaVolSurface::new(100.0, now, "EMPTY".to_string(), 0.05);

        let base_date = now.date_naive();
        let exp1 = base_date + chrono::Duration::days(7);
        let exp2 = base_date + chrono::Duration::days(30);

        let strike_provider = StrikeSpaceSelection;
        let delta_provider = DeltaSpaceSelection;

        // Empty surface should return None
        assert!(strike_provider.get_iv_pair(&empty_surface, 0.50, exp1, exp2, true).is_none());
        assert!(delta_provider.get_iv_pair(&empty_surface, 0.50, exp1, exp2, true).is_none());
    }
}
