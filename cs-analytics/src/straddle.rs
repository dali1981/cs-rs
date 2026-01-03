// Straddle price computation for expected move analysis
//
// Pure computational service - no I/O dependencies

use chrono::NaiveDate;
use std::collections::HashMap;

use crate::AtmMethod;

/// Straddle price at a specific expiration
#[derive(Debug, Clone)]
pub struct StraddlePrice {
    pub strike: f64,
    pub call_price: f64,
    pub put_price: f64,
    pub straddle_price: f64,
    pub expiration: NaiveDate,
    pub dte: i64,
}

/// Straddle price computer - pure computational service
///
/// Computes ATM straddle prices from option chains for expected move calculations.
pub struct StraddlePriceComputer;

impl StraddlePriceComputer {
    /// Compute straddle price from option chain
    ///
    /// # Arguments
    /// * `options` - Vector of (strike, expiration, price, is_call) tuples
    /// * `spot` - Current spot price
    /// * `pricing_date` - Current date for DTE calculation
    /// * `target_dte` - Optional specific DTE (None = nearest expiration)
    /// * `min_dte` - Minimum DTE to consider (default 1)
    /// * `atm_method` - Strike selection method
    ///
    /// # Returns
    /// StraddlePrice if both call and put are found at ATM strike
    pub fn compute_straddle(
        options: &[(f64, NaiveDate, f64, bool)], // (strike, expiration, price, is_call)
        spot: f64,
        pricing_date: NaiveDate,
        target_dte: Option<i64>,
        min_dte: i64,
        atm_method: AtmMethod,
    ) -> Option<StraddlePrice> {
        if options.is_empty() {
            return None;
        }

        // Group options by expiration
        let mut by_expiration: HashMap<NaiveDate, Vec<(f64, f64, bool)>> = HashMap::new();
        for &(strike, expiration, price, is_call) in options {
            let dte = (expiration - pricing_date).num_days();
            if dte >= min_dte {
                by_expiration
                    .entry(expiration)
                    .or_default()
                    .push((strike, price, is_call));
            }
        }

        if by_expiration.is_empty() {
            return None;
        }

        // Select expiration
        let selected_exp = match target_dte {
            Some(target) => {
                // Find closest expiration to target DTE
                by_expiration
                    .keys()
                    .min_by_key(|exp| {
                        let dte = (**exp - pricing_date).num_days();
                        (dte - target).abs()
                    })
                    .copied()?
            }
            None => {
                // Find nearest expiration
                by_expiration
                    .keys()
                    .min_by_key(|exp| (**exp - pricing_date).num_days())
                    .copied()?
            }
        };

        let exp_options = by_expiration.get(&selected_exp)?;
        let dte = (selected_exp - pricing_date).num_days();

        // Select ATM strike
        let atm_strike = Self::select_atm_strike(exp_options, spot, atm_method)?;

        // Find call and put at ATM strike
        let mut call_price: Option<f64> = None;
        let mut put_price: Option<f64> = None;

        for &(strike, price, is_call) in exp_options {
            if (strike - atm_strike).abs() < 1e-6 && price > 0.0 {
                if is_call {
                    call_price = Some(price);
                } else {
                    put_price = Some(price);
                }
            }
        }

        // Need both call and put for straddle
        match (call_price, put_price) {
            (Some(call), Some(put)) => Some(StraddlePrice {
                strike: atm_strike,
                call_price: call,
                put_price: put,
                straddle_price: call + put,
                expiration: selected_exp,
                dte,
            }),
            _ => None,
        }
    }

    /// Compute straddle for a specific DTE target
    ///
    /// Finds the expiration closest to target_dte and computes straddle there.
    pub fn compute_straddle_for_dte(
        options: &[(f64, NaiveDate, f64, bool)],
        spot: f64,
        pricing_date: NaiveDate,
        target_dte: i64,
        dte_tolerance: i64,
        atm_method: AtmMethod,
    ) -> Option<StraddlePrice> {
        if options.is_empty() {
            return None;
        }

        // Group by expiration
        let mut by_expiration: HashMap<NaiveDate, Vec<(f64, f64, bool)>> = HashMap::new();
        for &(strike, expiration, price, is_call) in options {
            let dte = (expiration - pricing_date).num_days();
            if dte > 0 {
                by_expiration
                    .entry(expiration)
                    .or_default()
                    .push((strike, price, is_call));
            }
        }

        // Find expiration closest to target within tolerance
        let best_exp = by_expiration
            .keys()
            .filter_map(|exp| {
                let dte = (*exp - pricing_date).num_days();
                let diff = (dte - target_dte).abs();
                if diff <= dte_tolerance {
                    Some((*exp, diff))
                } else {
                    None
                }
            })
            .min_by_key(|(_, diff)| *diff)
            .map(|(exp, _)| exp)?;

        let exp_options = by_expiration.get(&best_exp)?;
        let dte = (best_exp - pricing_date).num_days();

        let atm_strike = Self::select_atm_strike(exp_options, spot, atm_method)?;

        // Find call and put
        let mut call_price: Option<f64> = None;
        let mut put_price: Option<f64> = None;

        for &(strike, price, is_call) in exp_options {
            if (strike - atm_strike).abs() < 1e-6 && price > 0.0 {
                if is_call {
                    call_price = Some(price);
                } else {
                    put_price = Some(price);
                }
            }
        }

        match (call_price, put_price) {
            (Some(call), Some(put)) => Some(StraddlePrice {
                strike: atm_strike,
                call_price: call,
                put_price: put,
                straddle_price: call + put,
                expiration: best_exp,
                dte,
            }),
            _ => None,
        }
    }

    /// Calculate expected move as percentage of spot
    ///
    /// Expected Move (%) = Straddle Price / Spot × 100
    #[inline]
    pub fn expected_move(straddle_price: f64, spot: f64) -> f64 {
        if spot > 0.0 {
            (straddle_price / spot) * 100.0
        } else {
            0.0
        }
    }

    /// Calculate expected move using 85% rule
    ///
    /// Expected Move 85% = Straddle Price × 0.85 / Spot × 100
    ///
    /// This rule works well for short-dated options (1-7 DTE) around earnings
    /// where the straddle premium is mostly event-driven.
    #[inline]
    pub fn expected_move_85(straddle_price: f64, spot: f64) -> f64 {
        if spot > 0.0 {
            (straddle_price * 0.85 / spot) * 100.0
        } else {
            0.0
        }
    }

    /// Calculate expected 1-day move from annualized IV
    ///
    /// Expected 1-Day Move (%) = IV / sqrt(252) × 100
    ///                         ≈ IV × 6.30%
    #[inline]
    pub fn expected_1day_move_from_iv(annualized_iv: f64) -> f64 {
        annualized_iv / (252.0_f64).sqrt() * 100.0
    }

    /// Calculate expected move to expiration from IV
    ///
    /// Expected Move (%) = IV × sqrt(DTE / 365) × 100
    #[inline]
    pub fn expected_move_from_iv(annualized_iv: f64, dte: i64) -> f64 {
        if dte > 0 {
            annualized_iv * (dte as f64 / 365.0).sqrt() * 100.0
        } else {
            0.0
        }
    }

    /// Derive implied volatility from straddle price
    ///
    /// σ ≈ Straddle / (0.8 × Spot × sqrt(T))
    ///
    /// This is an approximation based on Black-Scholes straddle pricing.
    #[inline]
    pub fn iv_from_straddle(straddle_price: f64, spot: f64, dte: i64) -> Option<f64> {
        if spot <= 0.0 || dte <= 0 {
            return None;
        }

        let t = dte as f64 / 365.0;
        let denom = 0.8 * spot * t.sqrt();

        if denom > 0.0 {
            Some(straddle_price / denom)
        } else {
            None
        }
    }

    /// Select ATM strike based on method
    fn select_atm_strike(
        options: &[(f64, f64, bool)], // (strike, price, is_call)
        spot: f64,
        method: AtmMethod,
    ) -> Option<f64> {
        if options.is_empty() {
            return None;
        }

        // Get unique strikes
        let mut strikes: Vec<f64> = options.iter().map(|(s, _, _)| *s).collect();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        strikes.dedup();

        match method {
            AtmMethod::Closest => strikes
                .iter()
                .min_by(|a, b| {
                    let diff_a = (spot - **a).abs();
                    let diff_b = (spot - **b).abs();
                    diff_a.partial_cmp(&diff_b).unwrap()
                })
                .copied(),
            AtmMethod::BelowSpot => strikes
                .iter()
                .filter(|&&s| s <= spot)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .copied(),
            AtmMethod::AboveSpot => strikes
                .iter()
                .filter(|&&s| s >= spot)
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expected_move() {
        // Straddle = $10, Spot = $100 -> 10% expected move
        let em = StraddlePriceComputer::expected_move(10.0, 100.0);
        assert!((em - 10.0).abs() < 0.01);

        // With 85% rule -> 8.5%
        let em85 = StraddlePriceComputer::expected_move_85(10.0, 100.0);
        assert!((em85 - 8.5).abs() < 0.01);
    }

    #[test]
    fn test_expected_move_from_iv() {
        // 45% IV, 1-day move = 45% / sqrt(252) = ~2.84%
        let move_1d = StraddlePriceComputer::expected_1day_move_from_iv(0.45);
        assert!((move_1d - 2.835).abs() < 0.01);

        // 45% IV, 7-day move = 45% * sqrt(7/365) = ~6.23%
        let move_7d = StraddlePriceComputer::expected_move_from_iv(0.45, 7);
        assert!((move_7d - 6.23).abs() < 0.1);

        // 45% IV, 30-day move = 45% * sqrt(30/365) = ~12.9%
        let move_30d = StraddlePriceComputer::expected_move_from_iv(0.45, 30);
        assert!((move_30d - 12.9).abs() < 0.1);
    }

    #[test]
    fn test_iv_from_straddle() {
        // Straddle = $10, Spot = $100, 30 DTE
        // σ = 10 / (0.8 × 100 × sqrt(30/365)) = 10 / (0.8 × 100 × 0.287) = ~43.7%
        let iv = StraddlePriceComputer::iv_from_straddle(10.0, 100.0, 30);
        assert!(iv.is_some());
        assert!((iv.unwrap() - 0.436).abs() < 0.01);
    }

    #[test]
    fn test_compute_straddle() {
        let pricing_date = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        let exp = NaiveDate::from_ymd_opt(2025, 1, 22).unwrap(); // 7 DTE

        let options = vec![
            (100.0, exp, 5.0, true),  // Call at 100
            (100.0, exp, 4.5, false), // Put at 100
            (105.0, exp, 2.0, true),  // Call at 105
            (105.0, exp, 7.0, false), // Put at 105
        ];

        // Spot at 100 -> ATM strike = 100, straddle = 5 + 4.5 = 9.5
        let straddle =
            StraddlePriceComputer::compute_straddle(&options, 100.0, pricing_date, None, 1, AtmMethod::Closest);

        assert!(straddle.is_some());
        let s = straddle.unwrap();
        assert_eq!(s.strike, 100.0);
        assert_eq!(s.call_price, 5.0);
        assert_eq!(s.put_price, 4.5);
        assert_eq!(s.straddle_price, 9.5);
        assert_eq!(s.dte, 7);
    }

    #[test]
    fn test_compute_straddle_for_dte() {
        let pricing_date = NaiveDate::from_ymd_opt(2025, 1, 15).unwrap();
        let exp_7d = NaiveDate::from_ymd_opt(2025, 1, 22).unwrap();  // 7 DTE
        let exp_30d = NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(); // 30 DTE

        let options = vec![
            // 7 DTE
            (100.0, exp_7d, 5.0, true),
            (100.0, exp_7d, 4.5, false),
            // 30 DTE
            (100.0, exp_30d, 8.0, true),
            (100.0, exp_30d, 7.5, false),
        ];

        // Target 30 DTE with 5-day tolerance
        let straddle = StraddlePriceComputer::compute_straddle_for_dte(
            &options,
            100.0,
            pricing_date,
            30,
            5,
            AtmMethod::Closest,
        );

        assert!(straddle.is_some());
        let s = straddle.unwrap();
        assert_eq!(s.dte, 30);
        assert_eq!(s.straddle_price, 15.5); // 8 + 7.5
    }

    #[test]
    fn test_select_atm_strike() {
        let options = vec![
            (95.0, 2.0, true),
            (100.0, 5.0, true),
            (105.0, 3.0, true),
        ];

        // Spot at 102 -> closest is 100
        let atm = StraddlePriceComputer::select_atm_strike(&options, 102.0, AtmMethod::Closest);
        assert_eq!(atm, Some(100.0));

        // Spot at 102 -> below is 100
        let below = StraddlePriceComputer::select_atm_strike(&options, 102.0, AtmMethod::BelowSpot);
        assert_eq!(below, Some(100.0));

        // Spot at 102 -> above is 105
        let above = StraddlePriceComputer::select_atm_strike(&options, 102.0, AtmMethod::AboveSpot);
        assert_eq!(above, Some(105.0));
    }
}
