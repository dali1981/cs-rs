// ATM IV computation for earnings detection
//
// Pure computational service - no I/O dependencies

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;

use crate::{bs_implied_volatility, BSConfig};

/// ATM IV result for a specific maturity
#[derive(Debug, Clone)]
pub struct AtmIvResult {
    pub maturity_dte: i64,
    pub expiration: NaiveDate,
    pub atm_strike: f64,
    pub call_iv: Option<f64>,
    pub put_iv: Option<f64>,
    pub avg_iv: Option<f64>,
}

/// ATM strike selection method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtmMethod {
    /// Strike closest to spot (default)
    Closest,
    /// Strike immediately below spot
    BelowSpot,
    /// Strike immediately above spot
    AboveSpot,
}

impl Default for AtmMethod {
    fn default() -> Self {
        Self::Closest
    }
}

/// Option data point from chain
#[derive(Debug, Clone)]
pub struct OptionPoint {
    pub strike: f64,
    pub expiration: NaiveDate,
    pub price: f64,
    pub is_call: bool,
}

/// ATM IV at a specific expiration (used for constant-maturity interpolation)
#[derive(Debug, Clone)]
pub struct ExpirationIv {
    pub expiration: NaiveDate,
    pub dte: i64,
    pub atm_iv: f64,
    pub atm_strike: f64,
}

/// Constant-maturity IV interpolation result
#[derive(Debug, Clone)]
pub struct ConstantMaturityResult {
    pub target_dte: u32,
    /// Interpolated IV at exactly target_dte
    pub iv: f64,
    /// Whether this was interpolated (true) or extrapolated (false)
    pub is_interpolated: bool,
    /// Bracketing expirations used [lower_dte, upper_dte] or None if extrapolated
    pub bracket: Option<(i64, i64)>,
}

/// Constant-maturity IV interpolator
///
/// Interpolates in variance space (sigma^2 * T) to produce IVs at exact target DTEs.
/// This makes IV time series directly comparable across days.
pub struct ConstantMaturityInterpolator;

impl ConstantMaturityInterpolator {
    /// Interpolate ATM IV to exact target DTE using variance-space interpolation
    ///
    /// # Arguments
    /// * `term_structure` - ATM IVs at various expirations, must be sorted by DTE ascending
    /// * `target_dte` - Target DTE to interpolate to (e.g., 7, 14, 30, 60, 90)
    ///
    /// # Returns
    /// ConstantMaturityResult with interpolated IV, or None if insufficient data
    pub fn interpolate(
        term_structure: &[ExpirationIv],
        target_dte: u32,
    ) -> Option<ConstantMaturityResult> {
        if term_structure.is_empty() {
            return None;
        }

        let target_dte_i64 = target_dte as i64;

        // Find bracketing expirations
        let mut lower: Option<&ExpirationIv> = None;
        let mut upper: Option<&ExpirationIv> = None;

        for exp_iv in term_structure {
            if exp_iv.dte <= target_dte_i64 {
                lower = Some(exp_iv);
            } else if upper.is_none() {
                upper = Some(exp_iv);
                break;
            }
        }

        match (lower, upper) {
            (Some(l), Some(u)) => {
                // Interpolate in variance space
                let t1 = l.dte as f64 / 365.0;
                let t2 = u.dte as f64 / 365.0;
                let t_target = target_dte as f64 / 365.0;

                // Total variance: sigma^2 * T
                let var1 = l.atm_iv * l.atm_iv * t1;
                let var2 = u.atm_iv * u.atm_iv * t2;

                // Linear interpolation in variance
                let denom = t2 - t1;
                if denom.abs() < 1e-10 {
                    return Some(ConstantMaturityResult {
                        target_dte,
                        iv: l.atm_iv,
                        is_interpolated: true,
                        bracket: Some((l.dte, u.dte)),
                    });
                }

                let var_target = var1 + (var2 - var1) * (t_target - t1) / denom;

                // Back out IV: sigma = sqrt(variance / T)
                if t_target > 0.0 && var_target >= 0.0 {
                    let iv = (var_target / t_target).sqrt();
                    Some(ConstantMaturityResult {
                        target_dte,
                        iv,
                        is_interpolated: true,
                        bracket: Some((l.dte, u.dte)),
                    })
                } else {
                    None
                }
            }
            (Some(l), None) => {
                // Target is beyond longest expiration - extrapolate flat
                Some(ConstantMaturityResult {
                    target_dte,
                    iv: l.atm_iv,
                    is_interpolated: false,
                    bracket: None,
                })
            }
            (None, Some(u)) => {
                // Target is before shortest expiration - extrapolate flat
                Some(ConstantMaturityResult {
                    target_dte,
                    iv: u.atm_iv,
                    is_interpolated: false,
                    bracket: None,
                })
            }
            (None, None) => None,
        }
    }

    /// Interpolate multiple target DTEs
    pub fn interpolate_many(
        term_structure: &[ExpirationIv],
        targets: &[u32],
    ) -> Vec<ConstantMaturityResult> {
        targets
            .iter()
            .filter_map(|&target| Self::interpolate(term_structure, target))
            .collect()
    }
}

/// ATM IV computer - pure computational service
pub struct AtmIvComputer {
    pub bs_config: BSConfig,
}

impl AtmIvComputer {
    pub fn new() -> Self {
        Self {
            bs_config: BSConfig::default(),
        }
    }

    pub fn with_config(bs_config: BSConfig) -> Self {
        Self { bs_config }
    }

    /// Compute ATM IV for multiple maturity targets
    ///
    /// # Arguments
    /// * `options` - Vector of option data points
    /// * `spot_price` - Underlying spot price
    /// * `pricing_time` - Current time for TTM calculation
    /// * `maturity_targets` - Target DTEs to compute (e.g., [30, 60, 90])
    /// * `maturity_tolerance` - Tolerance window in days (e.g., 7)
    /// * `atm_method` - Method for selecting ATM strike
    pub fn compute_atm_ivs(
        &self,
        options: &[OptionPoint],
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        maturity_targets: &[u32],
        maturity_tolerance: u32,
        atm_method: AtmMethod,
    ) -> Vec<AtmIvResult> {
        let mut results = Vec::new();

        // Group options by expiration
        let mut by_expiration: HashMap<NaiveDate, Vec<&OptionPoint>> = HashMap::new();
        for opt in options {
            by_expiration.entry(opt.expiration).or_default().push(opt);
        }

        // For each maturity target, find closest expiration
        for &target_dte in maturity_targets {
            if let Some(result) = self.compute_atm_iv_for_target(
                &by_expiration,
                spot_price,
                pricing_time,
                target_dte,
                maturity_tolerance,
                atm_method,
            ) {
                results.push(result);
            }
        }

        results
    }

    /// Compute ATM IV for ALL available expirations
    ///
    /// Returns ATM IV for every expiration in the option chain, regardless of target DTEs.
    /// Used for constant-maturity interpolation.
    ///
    /// # Arguments
    /// * `options` - Vector of option data points
    /// * `spot_price` - Underlying spot price
    /// * `pricing_time` - Current time for TTM calculation
    /// * `min_dte` - Minimum DTE to include (e.g., 3 to avoid expiry effects)
    /// * `atm_method` - Method for selecting ATM strike
    ///
    /// # Returns
    /// Vector of ExpirationIv sorted by DTE ascending
    pub fn compute_all_atm_ivs(
        &self,
        options: &[OptionPoint],
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        min_dte: i64,
        atm_method: AtmMethod,
    ) -> Vec<ExpirationIv> {
        let pricing_date = pricing_time.date_naive();

        // Group options by expiration
        let mut by_expiration: HashMap<NaiveDate, Vec<&OptionPoint>> = HashMap::new();
        for opt in options {
            by_expiration.entry(opt.expiration).or_default().push(opt);
        }

        // Compute ATM IV for each expiration
        let mut results: Vec<ExpirationIv> = Vec::new();

        for (expiration, exp_options) in by_expiration {
            let dte = (expiration - pricing_date).num_days();
            if dte < min_dte {
                continue; // Skip expirations too close to expiry
            }

            // Select ATM strike
            let atm_strike = match self.select_atm_strike(&exp_options, spot_price, atm_method) {
                Some(s) => s,
                None => continue,
            };

            // Calculate TTM
            let ttm = dte as f64 / 365.25;

            // Find call and put at ATM strike and compute IV
            let mut call_iv: Option<f64> = None;
            let mut put_iv: Option<f64> = None;

            for opt in exp_options.iter() {
                if (opt.strike - atm_strike).abs() < 1e-6 && opt.price > 0.0 {
                    let iv = bs_implied_volatility(
                        opt.price,
                        spot_price,
                        opt.strike,
                        ttm,
                        opt.is_call,
                        &self.bs_config,
                    );

                    if let Some(iv_val) = iv {
                        if iv_val >= self.bs_config.min_iv && iv_val <= self.bs_config.max_iv {
                            if opt.is_call {
                                call_iv = Some(iv_val);
                            } else {
                                put_iv = Some(iv_val);
                            }
                        }
                    }
                }
            }

            // Compute average IV
            let avg_iv = match (call_iv, put_iv) {
                (Some(c), Some(p)) => Some((c + p) / 2.0),
                (Some(c), None) => Some(c),
                (None, Some(p)) => Some(p),
                (None, None) => None,
            };

            if let Some(iv) = avg_iv {
                results.push(ExpirationIv {
                    expiration,
                    dte,
                    atm_iv: iv,
                    atm_strike,
                });
            }
        }

        // Sort by DTE ascending
        results.sort_by_key(|r| r.dte);
        results
    }

    /// Compute ATM IV for a single maturity target
    fn compute_atm_iv_for_target(
        &self,
        by_expiration: &HashMap<NaiveDate, Vec<&OptionPoint>>,
        spot_price: f64,
        pricing_time: DateTime<Utc>,
        target_dte: u32,
        tolerance: u32,
        atm_method: AtmMethod,
    ) -> Option<AtmIvResult> {
        let pricing_date = pricing_time.date_naive();

        // Find expiration closest to target DTE within tolerance
        let mut best_expiration: Option<NaiveDate> = None;
        let mut best_dte_diff: i64 = i64::MAX;

        for &exp in by_expiration.keys() {
            let dte = (exp - pricing_date).num_days();
            if dte <= 0 {
                continue; // Skip expired
            }

            let diff = (dte - target_dte as i64).abs();
            if diff <= tolerance as i64 && diff < best_dte_diff {
                best_dte_diff = diff;
                best_expiration = Some(exp);
            }
        }

        let expiration = best_expiration?;
        let dte = (expiration - pricing_date).num_days();
        let options_at_exp = by_expiration.get(&expiration)?;

        // Select ATM strike
        let atm_strike = self.select_atm_strike(options_at_exp, spot_price, atm_method)?;

        // Calculate TTM (time to maturity in years)
        let ttm = dte as f64 / 365.25;

        // Find call and put at ATM strike
        let mut call_iv: Option<f64> = None;
        let mut put_iv: Option<f64> = None;

        for opt in options_at_exp.iter() {
            if (opt.strike - atm_strike).abs() < 1e-6 {
                if opt.price <= 0.0 {
                    continue;
                }

                let iv = bs_implied_volatility(
                    opt.price,
                    spot_price,
                    opt.strike,
                    ttm,
                    opt.is_call,
                    &self.bs_config,
                )?;

                // Skip unreasonable IVs
                if iv < self.bs_config.min_iv || iv > self.bs_config.max_iv {
                    continue;
                }

                if opt.is_call {
                    call_iv = Some(iv);
                } else {
                    put_iv = Some(iv);
                }
            }
        }

        // Compute average IV
        let avg_iv = match (call_iv, put_iv) {
            (Some(c), Some(p)) => Some((c + p) / 2.0),
            (Some(c), None) => Some(c),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };

        Some(AtmIvResult {
            maturity_dte: dte,
            expiration,
            atm_strike,
            call_iv,
            put_iv,
            avg_iv,
        })
    }

    /// Select ATM strike based on method
    fn select_atm_strike(
        &self,
        options: &[&OptionPoint],
        spot_price: f64,
        method: AtmMethod,
    ) -> Option<f64> {
        if options.is_empty() {
            return None;
        }

        // Get unique strikes
        let mut strikes: Vec<f64> = options.iter().map(|opt| opt.strike).collect();
        strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        strikes.dedup();

        match method {
            AtmMethod::Closest => {
                // Find strike with minimum distance to spot
                strikes
                    .iter()
                    .min_by(|a, b| {
                        let diff_a = (spot_price - **a).abs();
                        let diff_b = (spot_price - **b).abs();
                        diff_a.partial_cmp(&diff_b).unwrap()
                    })
                    .copied()
            }
            AtmMethod::BelowSpot => {
                // Find highest strike below spot
                strikes
                    .iter()
                    .filter(|&&s| s <= spot_price)
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .copied()
            }
            AtmMethod::AboveSpot => {
                // Find lowest strike above spot
                strikes
                    .iter()
                    .filter(|&&s| s >= spot_price)
                    .min_by(|a, b| a.partial_cmp(b).unwrap())
                    .copied()
            }
        }
    }
}

impl Default for AtmIvComputer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_select_atm_strike_closest() {
        let computer = AtmIvComputer::new();
        let exp = NaiveDate::from_ymd_opt(2025, 2, 21).unwrap();

        let options = vec![
            OptionPoint {
                strike: 100.0,
                expiration: exp,
                price: 5.0,
                is_call: true,
            },
            OptionPoint {
                strike: 105.0,
                expiration: exp,
                price: 3.0,
                is_call: true,
            },
            OptionPoint {
                strike: 110.0,
                expiration: exp,
                price: 1.0,
                is_call: true,
            },
        ];

        let refs: Vec<&OptionPoint> = options.iter().collect();

        // Spot at 103 -> closest is 105
        let atm = computer.select_atm_strike(&refs, 103.0, AtmMethod::Closest);
        assert_eq!(atm, Some(105.0));

        // Spot at 107 -> closest is 105
        let atm = computer.select_atm_strike(&refs, 107.0, AtmMethod::Closest);
        assert_eq!(atm, Some(105.0));
    }

    #[test]
    fn test_select_atm_strike_below_spot() {
        let computer = AtmIvComputer::new();
        let exp = NaiveDate::from_ymd_opt(2025, 2, 21).unwrap();

        let options = vec![
            OptionPoint {
                strike: 100.0,
                expiration: exp,
                price: 5.0,
                is_call: true,
            },
            OptionPoint {
                strike: 105.0,
                expiration: exp,
                price: 3.0,
                is_call: true,
            },
        ];

        let refs: Vec<&OptionPoint> = options.iter().collect();

        // Spot at 107 -> below is 105
        let atm = computer.select_atm_strike(&refs, 107.0, AtmMethod::BelowSpot);
        assert_eq!(atm, Some(105.0));

        // Spot at 102 -> below is 100
        let atm = computer.select_atm_strike(&refs, 102.0, AtmMethod::BelowSpot);
        assert_eq!(atm, Some(100.0));
    }

    #[test]
    fn test_select_atm_strike_above_spot() {
        let computer = AtmIvComputer::new();
        let exp = NaiveDate::from_ymd_opt(2025, 2, 21).unwrap();

        let options = vec![
            OptionPoint {
                strike: 100.0,
                expiration: exp,
                price: 5.0,
                is_call: true,
            },
            OptionPoint {
                strike: 105.0,
                expiration: exp,
                price: 3.0,
                is_call: true,
            },
        ];

        let refs: Vec<&OptionPoint> = options.iter().collect();

        // Spot at 98 -> above is 100
        let atm = computer.select_atm_strike(&refs, 98.0, AtmMethod::AboveSpot);
        assert_eq!(atm, Some(100.0));

        // Spot at 102 -> above is 105
        let atm = computer.select_atm_strike(&refs, 102.0, AtmMethod::AboveSpot);
        assert_eq!(atm, Some(105.0));
    }
}
