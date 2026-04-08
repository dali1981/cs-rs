// Generate ATM IV time series with minute-aligned prices
//
// Computes IV using time-aligned option and spot prices:
// - Each option's last trade timestamp → get spot at that timestamp
// - Compute IV with perfectly aligned data

use chrono::{DateTime, NaiveDate, Utc};
use polars::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;

use cs_analytics::{
    AtmIvComputer, AtmMethod, BSConfig,
    ConstantMaturityInterpolator, ExpirationIv,
    StraddlePriceComputer,
};
use cs_domain::{
    repositories::{EquityDataRepository, OptionsDataRepository},
    value_objects::{AtmIvConfig, AtmIvObservation, CallPut, HvConfig, IvInterpolationMethod, OptionBar},
    MarketTime, TradingDate,
};

/// Result of IV time series generation
#[derive(Debug)]
pub struct MinuteAlignedIvResult {
    pub symbol: String,
    pub observations: Vec<AtmIvObservation>,
    pub date_range: (NaiveDate, NaiveDate),
    pub total_days: usize,
    pub successful_days: usize,
}

/// Errors during IV time series generation
#[derive(Debug, thiserror::Error)]
pub enum MinuteAlignedIvError {
    #[error("No spot price for {symbol} at {time}")]
    NoSpotPrice { symbol: String, time: DateTime<Utc> },
    #[error("No options data for {symbol} on {date}")]
    NoOptionsData { symbol: String, date: NaiveDate },
    #[error("DataFrame column error: {0}")]
    DataFrameError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Polars error: {0}")]
    PolarsError(#[from] PolarsError),
}

/// Option point with timestamp
#[derive(Debug, Clone)]
struct TimestampedOption {
    strike: f64,
    expiration: NaiveDate,
    price: f64,
    is_call: bool,
    timestamp: DateTime<Utc>,
}

/// Use case for generating minute-aligned ATM IV time series
pub struct MinuteAlignedIvUseCase<E, O>
where
    E: EquityDataRepository,
    O: OptionsDataRepository,
{
    equity_repo: E,
    options_repo: O,
    atm_computer: AtmIvComputer,
}

impl<E, O> MinuteAlignedIvUseCase<E, O>
where
    E: EquityDataRepository,
    O: OptionsDataRepository,
{
    pub fn new(equity_repo: E, options_repo: O) -> Self {
        Self {
            equity_repo,
            options_repo,
            atm_computer: AtmIvComputer::new(),
        }
    }

    pub fn with_bs_config(equity_repo: E, options_repo: O, bs_config: BSConfig) -> Self {
        Self {
            equity_repo,
            options_repo,
            atm_computer: AtmIvComputer::with_config(bs_config),
        }
    }

    /// Generate ATM IV time series for a single symbol over date range
    pub async fn execute(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        config: &AtmIvConfig,
        hv_config: Option<&HvConfig>,
    ) -> Result<MinuteAlignedIvResult, MinuteAlignedIvError> {
        let mut observations = Vec::new();
        let mut successful_days = 0;
        let mut total_days = 0;

        // Iterate through date range
        let mut current_date = start_date;
        while current_date <= end_date {
            total_days += 1;

            // Attempt to compute observation for this date
            match self.compute_observation(symbol, current_date, config).await {
                Ok(Some(obs)) => {
                    observations.push(obs);
                    successful_days += 1;
                }
                Ok(None) => {
                    // No data available for this date, skip silently
                }
                Err(e) => {
                    eprintln!("Warning: Error on {}: {:?}", current_date, e);
                }
            }

            // Move to next day
            current_date = current_date
                .succ_opt()
                .ok_or_else(|| MinuteAlignedIvError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Date overflow",
                )))?;
        }

        // Enrich with historical volatility if requested
        if let Some(hv_cfg) = hv_config {
            self.enrich_with_hv(symbol, &mut observations, hv_cfg).await?;
        }

        Ok(MinuteAlignedIvResult {
            symbol: symbol.to_string(),
            observations,
            date_range: (start_date, end_date),
            total_days,
            successful_days,
        })
    }

    /// Compute single ATM IV observation for a specific date
    /// Uses minute-aligned pricing: each option's last trade time → spot at that time
    async fn compute_observation(
        &self,
        symbol: &str,
        date: NaiveDate,
        config: &AtmIvConfig,
    ) -> Result<Option<AtmIvObservation>, MinuteAlignedIvError> {
        // Get minute-level option bars for the day
        let chain = match self.options_repo.get_option_minute_bars(symbol, date).await {
            Ok(bars) => bars,
            Err(_) => return Ok(None), // No options data
        };

        if chain.is_empty() {
            return Ok(None);
        }

        // Extract timestamped options (last trade per contract)
        let options_with_timestamps = self.extract_timestamped_options(&chain)?;

        if options_with_timestamps.is_empty() {
            return Ok(None);
        }

        // Compute IV for each contract with time-aligned spot price
        let mut iv_results: HashMap<(i64, NaiveDate), Vec<(f64, f64, bool)>> = HashMap::new();

        for opt in &options_with_timestamps {
            // Get spot price at the option's trade timestamp
            let spot_price = match self.equity_repo.get_spot_price(symbol, opt.timestamp).await {
                Ok(sp) => sp,
                Err(_) => continue, // Skip if no spot price available
            };

            // Calculate time to maturity
            let pricing_date = opt.timestamp.date_naive();
            let dte = (opt.expiration - pricing_date).num_days();
            if dte <= 0 {
                continue; // Skip expired
            }
            let ttm = dte as f64 / 365.25;

            // Note: Do NOT filter by target maturity here
            // For constant-maturity interpolation, we need ALL available expirations
            // to build a complete term structure. Filtering happens later in
            // build_term_structure (by min_dte) and during interpolation (by target matching)

            // Compute IV using Black-Scholes
            if opt.price <= 0.0 {
                continue;
            }

            let spot_f64 = spot_price.to_f64();
            let iv = cs_analytics::bs_implied_volatility(
                opt.price,
                spot_f64,
                opt.strike,
                ttm,
                opt.is_call,
                &self.atm_computer.bs_config,
            );

            if let Some(iv_value) = iv {
                // Skip unreasonable IVs
                if iv_value < config.iv_min_bound || iv_value > config.iv_max_bound {
                    continue;
                }

                // Store (strike, IV, is_call) grouped by (dte, expiration)
                iv_results
                    .entry((dte, opt.expiration))
                    .or_default()
                    .push((opt.strike, iv_value, opt.is_call));
            }
        }

        // Select ATM strikes for each maturity and compute average IV
        let mut obs = AtmIvObservation::new(
            symbol.to_string(),
            date,
            // Use a representative spot (last available for the day)
            self.get_representative_spot(symbol, date).await?,
        );

        // Convert ATM method
        let atm_method = match config.atm_strike_method {
            cs_domain::value_objects::AtmMethod::Closest => AtmMethod::Closest,
            cs_domain::value_objects::AtmMethod::BelowSpot => AtmMethod::BelowSpot,
            cs_domain::value_objects::AtmMethod::AboveSpot => AtmMethod::AboveSpot,
        };

        // Find and compute nearest expiration IV (>3 DTE to avoid expiry effects)
        let spot_f64 = obs.spot.to_string().parse::<f64>().unwrap_or(0.0);
        if let Some((nearest_dte, nearest_exp)) = iv_results
            .keys()
            .filter(|(dte, _)| *dte > 3) // Only consider >3 DTE
            .min_by_key(|(dte, _)| *dte)
        {
            let contracts = &iv_results[&(*nearest_dte, *nearest_exp)];
            let strikes: Vec<f64> = contracts.iter().map(|(s, _, _)| *s).collect();

            if let Some(strike) = self.select_atm_strike(&strikes, spot_f64, atm_method) {
                let mut call_iv: Option<f64> = None;
                let mut put_iv: Option<f64> = None;

                for (s, iv, is_call) in contracts {
                    if (s - strike).abs() < 1e-6 {
                        if *is_call {
                            call_iv = Some(*iv);
                        } else {
                            put_iv = Some(*iv);
                        }
                    }
                }

                let avg_iv = match (call_iv, put_iv) {
                    (Some(c), Some(p)) => Some((c + p) / 2.0),
                    (Some(c), None) => Some(c),
                    (None, Some(p)) => Some(p),
                    (None, None) => None,
                };

                obs.atm_iv_nearest = avg_iv;
                obs.nearest_dte = Some(*nearest_dte);
            }
        }

        // Branch based on interpolation method
        let spot_f64 = obs.spot.to_string().parse::<f64>().unwrap_or(0.0);
        match config.interpolation_method {
            IvInterpolationMethod::Rolling => {
                self.compute_rolling_ivs(&mut obs, &iv_results, spot_f64, config, atm_method);
            }
            IvInterpolationMethod::ConstantMaturity => {
                // Build term structure and compute CM IVs
                let term_structure = self.build_term_structure(&iv_results, spot_f64, config, atm_method);
                self.compute_constant_maturity_ivs(&mut obs, &term_structure, config);

                // Also compute rolling for comparison
                self.compute_rolling_ivs(&mut obs, &iv_results, spot_f64, config, atm_method);
            }
        }

        // Compute straddle prices and expected moves
        self.compute_straddle_and_expected_move(&mut obs, &options_with_timestamps, date, atm_method);

        // Calculate term spreads
        obs.calculate_spreads();

        Ok(Some(obs))
    }

    /// Extract timestamped options from minute bars slice.
    /// Groups by contract (strike, expiration, option_type) and takes the latest bar per group.
    fn extract_timestamped_options(
        &self,
        chain: &[OptionBar],
    ) -> Result<Vec<TimestampedOption>, MinuteAlignedIvError> {
        // For each contract key, keep the bar with the latest timestamp
        let mut latest: std::collections::HashMap<(u64, NaiveDate, bool), (DateTime<Utc>, &OptionBar)>
            = std::collections::HashMap::new();

        for bar in chain {
            let ts = match bar.timestamp {
                Some(ts) => ts,
                None => continue,
            };
            if bar.close.map_or(true, |c| c <= 0.0) || bar.strike <= 0.0 {
                continue;
            }
            let key = (bar.strike.to_bits(), bar.expiration, matches!(bar.option_type, CallPut::Call));
            let should_update = latest.get(&key).map_or(true, |(prev_ts, _)| ts > *prev_ts);
            if should_update {
                latest.insert(key, (ts, bar));
            }
        }

        let options: Vec<TimestampedOption> = latest
            .into_values()
            .filter_map(|(ts, bar)| {
                let close = bar.close.filter(|&c| c > 0.0)?;
                Some(TimestampedOption {
                    strike: bar.strike,
                    expiration: bar.expiration,
                    price: close,
                    is_call: matches!(bar.option_type, CallPut::Call),
                    timestamp: ts,
                })
            })
            .collect();

        Ok(options)
    }

    /// Get representative spot price for the day (last available)
    async fn get_representative_spot(
        &self,
        symbol: &str,
        date: NaiveDate,
    ) -> Result<rust_decimal::Decimal, MinuteAlignedIvError> {
        // Use EOD time as representative
        let eod_time = TradingDate::from_naive_date(date)
            .with_time(&MarketTime::new(16, 0))
            .to_datetime_utc();

        let spot = self
            .equity_repo
            .get_spot_price(symbol, eod_time)
            .await
            .map_err(|_| MinuteAlignedIvError::NoSpotPrice {
                symbol: symbol.to_string(),
                time: eod_time,
            })?;

        Ok(spot.value)
    }

    /// Select ATM strike based on method
    fn select_atm_strike(&self, strikes: &[f64], spot_price: f64, method: AtmMethod) -> Option<f64> {
        if strikes.is_empty() {
            return None;
        }

        let mut unique_strikes = strikes.to_vec();
        unique_strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique_strikes.dedup();

        match method {
            AtmMethod::Closest => unique_strikes
                .iter()
                .min_by(|a, b| {
                    let diff_a = (spot_price - **a).abs();
                    let diff_b = (spot_price - **b).abs();
                    diff_a.partial_cmp(&diff_b).unwrap()
                })
                .copied(),
            AtmMethod::BelowSpot => unique_strikes
                .iter()
                .filter(|&&s| s <= spot_price)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .copied(),
            AtmMethod::AboveSpot => unique_strikes
                .iter()
                .filter(|&&s| s >= spot_price)
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .copied(),
        }
    }

    /// Compute rolling TTE IVs from iv_results HashMap
    fn compute_rolling_ivs(
        &self,
        obs: &mut AtmIvObservation,
        iv_results: &HashMap<(i64, NaiveDate), Vec<(f64, f64, bool)>>,
        spot_price: f64,
        config: &AtmIvConfig,
        atm_method: AtmMethod,
    ) {
        for (dte, expiration) in iv_results.keys() {
            let contracts = &iv_results[&(*dte, *expiration)];

            // Select ATM strike
            let strikes: Vec<f64> = contracts.iter().map(|(s, _, _)| *s).collect();
            let atm_strike = self.select_atm_strike(&strikes, spot_price, atm_method);

            if let Some(strike) = atm_strike {
                // Average call and put IV at this strike
                let mut call_iv: Option<f64> = None;
                let mut put_iv: Option<f64> = None;

                for (s, iv, is_call) in contracts {
                    if (s - strike).abs() < 1e-6 {
                        if *is_call {
                            call_iv = Some(*iv);
                        } else {
                            put_iv = Some(*iv);
                        }
                    }
                }

                let avg_iv = match (call_iv, put_iv) {
                    (Some(c), Some(p)) => Some((c + p) / 2.0),
                    (Some(c), None) => Some(c),
                    (None, Some(p)) => Some(p),
                    (None, None) => None,
                };

                // Assign to observation based on target DTE
                if (*dte - 30).abs() <= config.maturity_tolerance as i64 {
                    obs.atm_iv_30d = avg_iv;
                } else if (*dte - 60).abs() <= config.maturity_tolerance as i64 {
                    obs.atm_iv_60d = avg_iv;
                } else if (*dte - 90).abs() <= config.maturity_tolerance as i64 {
                    obs.atm_iv_90d = avg_iv;
                }
            }
        }
    }

    /// Build ExpirationIv term structure from iv_results HashMap
    fn build_term_structure(
        &self,
        iv_results: &HashMap<(i64, NaiveDate), Vec<(f64, f64, bool)>>,
        spot_price: f64,
        config: &AtmIvConfig,
        atm_method: AtmMethod,
    ) -> Vec<ExpirationIv> {
        let mut term_structure = Vec::new();

        for ((dte, expiration), contracts) in iv_results {
            // Filter out expirations below min_dte
            if *dte <= config.min_dte {
                continue;
            }

            // Select ATM strike
            let strikes: Vec<f64> = contracts.iter().map(|(s, _, _)| *s).collect();
            let atm_strike = self.select_atm_strike(&strikes, spot_price, atm_method);

            if let Some(strike) = atm_strike {
                // Average call and put IV at this strike
                let mut call_iv: Option<f64> = None;
                let mut put_iv: Option<f64> = None;

                for (s, iv, is_call) in contracts {
                    if (s - strike).abs() < 1e-6 {
                        if *is_call {
                            call_iv = Some(*iv);
                        } else {
                            put_iv = Some(*iv);
                        }
                    }
                }

                let avg_iv = match (call_iv, put_iv) {
                    (Some(c), Some(p)) => (c + p) / 2.0,
                    (Some(c), None) => c,
                    (None, Some(p)) => p,
                    (None, None) => continue,
                };

                term_structure.push(ExpirationIv {
                    expiration: *expiration,
                    dte: *dte,
                    atm_iv: avg_iv,
                    atm_strike: strike,
                });
            }
        }

        // Sort by DTE ascending
        term_structure.sort_by_key(|e| e.dte);
        term_structure
    }

    /// Compute constant-maturity IVs via variance interpolation
    fn compute_constant_maturity_ivs(
        &self,
        obs: &mut AtmIvObservation,
        term_structure: &[ExpirationIv],
        config: &AtmIvConfig,
    ) {
        if term_structure.is_empty() {
            return;
        }

        obs.cm_num_expirations = Some(term_structure.len());

        // Interpolate to target maturities
        let cm_results = ConstantMaturityInterpolator::interpolate_many(
            term_structure,
            &config.maturity_targets,
        );

        let mut any_interpolated = false;
        for result in cm_results {
            if result.is_interpolated {
                any_interpolated = true;
            }

            match result.target_dte {
                7 => obs.cm_iv_7d = Some(result.iv),
                14 => obs.cm_iv_14d = Some(result.iv),
                21 => obs.cm_iv_21d = Some(result.iv),
                30 => obs.cm_iv_30d = Some(result.iv),
                60 => obs.cm_iv_60d = Some(result.iv),
                90 => obs.cm_iv_90d = Some(result.iv),
                _ => {}
            }
        }

        obs.cm_interpolated = Some(any_interpolated);
    }

    /// Compute straddle price and expected move from timestamped options
    fn compute_straddle_and_expected_move(
        &self,
        obs: &mut AtmIvObservation,
        options: &[TimestampedOption],
        date: NaiveDate,
        atm_method: AtmMethod,
    ) {
        if options.is_empty() {
            return;
        }

        let spot_f64 = obs.spot.to_string().parse::<f64>().unwrap_or(0.0);
        if spot_f64 <= 0.0 {
            return;
        }

        // Convert to format expected by StraddlePriceComputer
        let option_data: Vec<(f64, NaiveDate, f64, bool)> = options
            .iter()
            .map(|o| (o.strike, o.expiration, o.price, o.is_call))
            .collect();

        // Convert ATM method
        let straddle_atm_method = match atm_method {
            AtmMethod::Closest => cs_analytics::AtmMethod::Closest,
            AtmMethod::BelowSpot => cs_analytics::AtmMethod::BelowSpot,
            AtmMethod::AboveSpot => cs_analytics::AtmMethod::AboveSpot,
        };

        // Compute straddle for nearest expiration
        if let Some(straddle_nearest) = StraddlePriceComputer::compute_straddle(
            &option_data,
            spot_f64,
            date,
            None, // No target DTE, use nearest
            1,    // Min DTE
            straddle_atm_method,
        ) {
            obs.straddle_price_nearest = Some(straddle_nearest.straddle_price);
            obs.expected_move_pct = Some(StraddlePriceComputer::expected_move(
                straddle_nearest.straddle_price,
                spot_f64,
            ));
            obs.expected_move_85_pct = Some(StraddlePriceComputer::expected_move_85(
                straddle_nearest.straddle_price,
                spot_f64,
            ));
        }

        // Compute straddle for 30-day options (with 7-day tolerance)
        if let Some(straddle_30d) = StraddlePriceComputer::compute_straddle_for_dte(
            &option_data,
            spot_f64,
            date,
            30,  // Target 30 DTE
            7,   // Tolerance
            straddle_atm_method,
        ) {
            obs.straddle_price_30d = Some(straddle_30d.straddle_price);
            obs.expected_move_30d_pct = Some(StraddlePriceComputer::expected_move(
                straddle_30d.straddle_price,
                spot_f64,
            ));
        }
    }

    /// Save observations to Parquet file
    pub fn save_to_parquet(
        result: &MinuteAlignedIvResult,
        output_path: &PathBuf,
    ) -> Result<(), MinuteAlignedIvError> {
        // Build DataFrame from observations
        let symbols: Vec<String> = result.observations.iter().map(|o| o.symbol.clone()).collect();
        let dates: Vec<i32> = result
            .observations
            .iter()
            .map(|o| {
                o.date
                    .signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
                    .num_days() as i32
            })
            .collect();
        let spots: Vec<f64> = result
            .observations
            .iter()
            .map(|o| o.spot.to_string().parse::<f64>().unwrap_or(0.0))
            .collect();

        // Rolling TTE fields (existing)
        let iv_nearest: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_nearest).collect();
        let nearest_dte: Vec<Option<i64>> = result.observations.iter().map(|o| o.nearest_dte).collect();
        let iv_30d: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_30d).collect();
        let iv_60d: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_60d).collect();
        let iv_90d: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_90d).collect();
        let spread_30_60: Vec<Option<f64>> = result
            .observations
            .iter()
            .map(|o| o.term_spread_30_60)
            .collect();
        let spread_30_90: Vec<Option<f64>> = result
            .observations
            .iter()
            .map(|o| o.term_spread_30_90)
            .collect();

        // Constant-Maturity fields (new)
        let cm_iv_7d: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_iv_7d).collect();
        let cm_iv_14d: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_iv_14d).collect();
        let cm_iv_21d: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_iv_21d).collect();
        let cm_iv_30d: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_iv_30d).collect();
        let cm_iv_60d: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_iv_60d).collect();
        let cm_iv_90d: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_iv_90d).collect();
        let cm_interpolated: Vec<Option<bool>> = result.observations.iter().map(|o| o.cm_interpolated).collect();
        let cm_num_expirations: Vec<Option<u32>> = result.observations.iter().map(|o| o.cm_num_expirations.map(|n| n as u32)).collect();
        let cm_spread_7_30: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_spread_7_30).collect();
        let cm_spread_30_60: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_spread_30_60).collect();
        let cm_spread_30_90: Vec<Option<f64>> = result.observations.iter().map(|o| o.cm_spread_30_90).collect();

        // HV columns
        let hv_10d: Vec<Option<f64>> = result.observations.iter().map(|o| o.hv_10d).collect();
        let hv_20d: Vec<Option<f64>> = result.observations.iter().map(|o| o.hv_20d).collect();
        let hv_30d: Vec<Option<f64>> = result.observations.iter().map(|o| o.hv_30d).collect();
        let hv_60d: Vec<Option<f64>> = result.observations.iter().map(|o| o.hv_60d).collect();
        let iv_hv_spread_30d: Vec<Option<f64>> = result.observations.iter().map(|o| o.iv_hv_spread_30d).collect();

        // Expected Move columns (from straddle)
        let straddle_nearest: Vec<Option<f64>> = result.observations.iter().map(|o| o.straddle_price_nearest).collect();
        let expected_move_pct: Vec<Option<f64>> = result.observations.iter().map(|o| o.expected_move_pct).collect();
        let expected_move_85_pct: Vec<Option<f64>> = result.observations.iter().map(|o| o.expected_move_85_pct).collect();
        let straddle_30d: Vec<Option<f64>> = result.observations.iter().map(|o| o.straddle_price_30d).collect();
        let expected_move_30d_pct: Vec<Option<f64>> = result.observations.iter().map(|o| o.expected_move_30d_pct).collect();

        let df = DataFrame::new(vec![
            Series::new("symbol", symbols),
            Series::new("date", dates),
            Series::new("spot", spots),
            // Rolling TTE columns
            Series::new("atm_iv_nearest", iv_nearest),
            Series::new("nearest_dte", nearest_dte),
            Series::new("atm_iv_30d", iv_30d),
            Series::new("atm_iv_60d", iv_60d),
            Series::new("atm_iv_90d", iv_90d),
            Series::new("term_spread_30_60", spread_30_60),
            Series::new("term_spread_30_90", spread_30_90),
            // Constant-Maturity columns
            Series::new("cm_iv_7d", cm_iv_7d),
            Series::new("cm_iv_14d", cm_iv_14d),
            Series::new("cm_iv_21d", cm_iv_21d),
            Series::new("cm_iv_30d", cm_iv_30d),
            Series::new("cm_iv_60d", cm_iv_60d),
            Series::new("cm_iv_90d", cm_iv_90d),
            Series::new("cm_interpolated", cm_interpolated),
            Series::new("cm_num_expirations", cm_num_expirations),
            Series::new("cm_spread_7_30", cm_spread_7_30),
            Series::new("cm_spread_30_60", cm_spread_30_60),
            Series::new("cm_spread_30_90", cm_spread_30_90),
            // Historical Volatility columns
            Series::new("hv_10d", hv_10d),
            Series::new("hv_20d", hv_20d),
            Series::new("hv_30d", hv_30d),
            Series::new("hv_60d", hv_60d),
            Series::new("iv_hv_spread_30d", iv_hv_spread_30d),
            // Expected Move columns
            Series::new("straddle_price_nearest", straddle_nearest),
            Series::new("expected_move_pct", expected_move_pct),
            Series::new("expected_move_85_pct", expected_move_85_pct),
            Series::new("straddle_price_30d", straddle_30d),
            Series::new("expected_move_30d_pct", expected_move_30d_pct),
        ])?;

        // Write to parquet
        let mut file = std::fs::File::create(output_path)?;
        ParquetWriter::new(&mut file).finish(&mut df.clone())?;

        Ok(())
    }

    /// Collect daily close prices for the date range covered by observations
    async fn collect_daily_closes(
        &self,
        symbol: &str,
        observations: &[AtmIvObservation],
        lookback_days: usize,
    ) -> Result<HashMap<NaiveDate, f64>, MinuteAlignedIvError> {
        if observations.is_empty() {
            return Ok(HashMap::new());
        }

        // Determine date range with lookback buffer
        let first_date = observations.first().unwrap().date;
        let last_date = observations.last().unwrap().date;

        // Add lookback buffer to start date
        let start_with_buffer = first_date - chrono::Duration::days(lookback_days as i64);

        let mut closes = HashMap::new();
        let mut current_date = start_with_buffer;

        while current_date <= last_date {
            // Get bars for this date
            if let Ok(bars) = self.equity_repo.get_bars(symbol, current_date).await {
                if let Some(last_close) = bars.last().map(|b| b.close) {
                    closes.insert(current_date, last_close);
                }
            }

            // Move to next day
            current_date = current_date
                .succ_opt()
                .ok_or_else(|| MinuteAlignedIvError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Date overflow",
                )))?;
        }

        Ok(closes)
    }

    /// Enrich observations with historical volatility calculations
    async fn enrich_with_hv(
        &self,
        symbol: &str,
        observations: &mut [AtmIvObservation],
        hv_config: &HvConfig,
    ) -> Result<(), MinuteAlignedIvError> {
        // Determine maximum lookback needed
        let max_window = hv_config.windows.iter().max().copied().unwrap_or(60);

        // Collect all daily close prices with lookback buffer
        let daily_closes = self.collect_daily_closes(symbol, observations, max_window + 10).await?;

        // Build sorted price history
        let mut dates: Vec<NaiveDate> = daily_closes.keys().copied().collect();
        dates.sort();

        // For each observation, compute HV
        for obs in observations.iter_mut() {
            // Find all dates up to and including this observation's date
            let prices: Vec<f64> = dates.iter()
                .filter(|&&d| d <= obs.date)
                .filter_map(|d| daily_closes.get(d).copied())
                .collect();

            // Skip if not enough data
            if prices.len() < hv_config.min_data_points {
                continue;
            }

            // Compute HV for each window
            for &window in &hv_config.windows {
                if prices.len() < window + 1 {
                    continue;
                }

                let hv = cs_analytics::realized_volatility(
                    &prices,
                    window,
                    hv_config.annualization_factor,
                );

                // Assign to appropriate field
                match window {
                    10 => obs.hv_10d = hv,
                    20 => obs.hv_20d = hv,
                    30 => obs.hv_30d = hv,
                    60 => obs.hv_60d = hv,
                    _ => {} // Other windows not stored
                }
            }

            // Calculate IV-HV spread if we have both values
            if let (Some(cm_iv_30d), Some(hv_30d)) = (obs.cm_iv_30d, obs.hv_30d) {
                obs.iv_hv_spread_30d = Some(cm_iv_30d - hv_30d);
            }
        }

        Ok(())
    }
}
