// Generate ATM IV time series for earnings detection
//
// Use case for computing daily ATM IV observations over a date range

use chrono::NaiveDate;
use polars::prelude::*;
use std::path::PathBuf;

use cs_analytics::{AtmIvComputer, AtmMethod, BSConfig, OptionPoint};
use cs_domain::{
    repositories::{EquityDataRepository, OptionsDataRepository},
    value_objects::{AtmIvConfig, AtmIvObservation},
    MarketTime, TradingDate,
};

/// Result of IV time series generation
#[derive(Debug)]
pub struct IvTimeSeriesResult {
    pub symbol: String,
    pub observations: Vec<AtmIvObservation>,
    pub date_range: (NaiveDate, NaiveDate),
    pub total_days: usize,
    pub successful_days: usize,
}

/// Errors during IV time series generation
#[derive(Debug, thiserror::Error)]
pub enum IvTimeSeriesError {
    #[error("No spot price for {symbol} on {date}")]
    NoSpotPrice { symbol: String, date: NaiveDate },
    #[error("No options data for {symbol} on {date}")]
    NoOptionsData { symbol: String, date: NaiveDate },
    #[error("DataFrame column error: {0}")]
    DataFrameError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Polars error: {0}")]
    PolarsError(#[from] PolarsError),
}

/// Use case for generating ATM IV time series
pub struct GenerateIvTimeSeriesUseCase<E, O>
where
    E: EquityDataRepository,
    O: OptionsDataRepository,
{
    equity_repo: E,
    options_repo: O,
    atm_computer: AtmIvComputer,
    market_close: MarketTime,
}

impl<E, O> GenerateIvTimeSeriesUseCase<E, O>
where
    E: EquityDataRepository,
    O: OptionsDataRepository,
{
    pub fn new(equity_repo: E, options_repo: O) -> Self {
        Self {
            equity_repo,
            options_repo,
            atm_computer: AtmIvComputer::new(),
            market_close: MarketTime::new(16, 0),
        }
    }

    pub fn with_bs_config(equity_repo: E, options_repo: O, bs_config: BSConfig) -> Self {
        Self {
            equity_repo,
            options_repo,
            atm_computer: AtmIvComputer::with_config(bs_config),
            market_close: MarketTime::new(16, 0),
        }
    }

    /// Generate ATM IV time series for a single symbol over date range
    pub async fn execute(
        &self,
        symbol: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        config: &AtmIvConfig,
    ) -> Result<IvTimeSeriesResult, IvTimeSeriesError> {
        let mut observations = Vec::new();
        let mut successful_days = 0;
        let mut total_days = 0;

        // Iterate through date range (trading days only)
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
                .ok_or_else(|| IvTimeSeriesError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Date overflow",
                )))?;
        }

        Ok(IvTimeSeriesResult {
            symbol: symbol.to_string(),
            observations,
            date_range: (start_date, end_date),
            total_days,
            successful_days,
        })
    }

    /// Compute single ATM IV observation for a specific date
    async fn compute_observation(
        &self,
        symbol: &str,
        date: NaiveDate,
        config: &AtmIvConfig,
    ) -> Result<Option<AtmIvObservation>, IvTimeSeriesError> {
        // Build pricing time (EOD = 16:00 Eastern)
        let pricing_timestamp = TradingDate::from_naive_date(date).with_time(&self.market_close);
        let pricing_time = pricing_timestamp.to_datetime_utc();

        // Get spot price at EOD
        let spot_price = match self.equity_repo.get_spot_price(symbol, pricing_time).await {
            Ok(sp) => sp,
            Err(_) => return Ok(None), // No data for this date
        };

        // Get option chain for this date
        let chain_df = match self.options_repo.get_option_bars(symbol, date).await {
            Ok(df) => df,
            Err(_) => return Ok(None), // No options data
        };

        if chain_df.height() == 0 {
            return Ok(None);
        }

        // Convert DataFrame to OptionPoints
        let options = self.dataframe_to_options(&chain_df)?;

        if options.is_empty() {
            return Ok(None);
        }

        // Compute ATM IVs for all maturity targets
        let atm_method = match config.atm_strike_method {
            cs_domain::value_objects::AtmMethod::Closest => AtmMethod::Closest,
            cs_domain::value_objects::AtmMethod::BelowSpot => AtmMethod::BelowSpot,
            cs_domain::value_objects::AtmMethod::AboveSpot => AtmMethod::AboveSpot,
        };

        let results = self.atm_computer.compute_atm_ivs(
            &options,
            spot_price.to_f64(),
            pricing_time,
            &config.maturity_targets,
            config.maturity_tolerance,
            atm_method,
        );

        // Build observation
        let mut obs = AtmIvObservation::new(symbol.to_string(), date, spot_price.value);

        // Map results to observation fields based on target DTE
        for result in results {
            let target_dte = result.maturity_dte;

            // Match to closest target
            if (target_dte - 30).abs() <= config.maturity_tolerance as i64 {
                obs.atm_iv_30d = result.avg_iv;
            } else if (target_dte - 60).abs() <= config.maturity_tolerance as i64 {
                obs.atm_iv_60d = result.avg_iv;
            } else if (target_dte - 90).abs() <= config.maturity_tolerance as i64 {
                obs.atm_iv_90d = result.avg_iv;
            }
        }

        // Calculate term spreads
        obs.calculate_spreads();

        Ok(Some(obs))
    }

    /// Convert Polars DataFrame to vector of OptionPoints
    fn dataframe_to_options(&self, df: &DataFrame) -> Result<Vec<OptionPoint>, IvTimeSeriesError> {
        let strikes = df
            .column("strike")
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?
            .f64()
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?;

        let expirations = df
            .column("expiration")
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?
            .date()
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?;

        let closes = df
            .column("close")
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?
            .f64()
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?;

        let option_types = df
            .column("option_type")
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?
            .str()
            .map_err(|e| IvTimeSeriesError::DataFrameError(e.to_string()))?;

        let mut options = Vec::new();

        for i in 0..df.height() {
            let (strike, exp_days, close, opt_type) =
                match (strikes.get(i), expirations.get(i), closes.get(i), option_types.get(i)) {
                    (Some(s), Some(e), Some(c), Some(t)) => (s, e, c, t),
                    _ => continue,
                };

            if close <= 0.0 || strike <= 0.0 {
                continue;
            }

            let expiration = TradingDate::from_polars_date(exp_days).to_naive_date();
            let is_call = opt_type == "call";

            options.push(OptionPoint {
                strike,
                expiration,
                price: close,
                is_call,
            });
        }

        Ok(options)
    }

    /// Save observations to Parquet file
    pub fn save_to_parquet(
        result: &IvTimeSeriesResult,
        output_path: &PathBuf,
    ) -> Result<(), IvTimeSeriesError> {
        // Build DataFrame from observations
        let symbols: Vec<String> = result.observations.iter().map(|o| o.symbol.clone()).collect();
        let dates: Vec<i32> = result
            .observations
            .iter()
            .map(|o| {
                // Convert NaiveDate to days since Unix epoch
                o.date.signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days() as i32
            })
            .collect();
        let spots: Vec<f64> = result.observations.iter().map(|o| o.spot.to_string().parse::<f64>().unwrap_or(0.0)).collect();
        let iv_30d: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_30d).collect();
        let iv_60d: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_60d).collect();
        let iv_90d: Vec<Option<f64>> = result.observations.iter().map(|o| o.atm_iv_90d).collect();
        let spread_30_60: Vec<Option<f64>> =
            result.observations.iter().map(|o| o.term_spread_30_60).collect();
        let spread_30_90: Vec<Option<f64>> =
            result.observations.iter().map(|o| o.term_spread_30_90).collect();

        let df = DataFrame::new(vec![
            Series::new("symbol", symbols),
            Series::new("date", dates),
            Series::new("spot", spots),
            Series::new("atm_iv_30d", iv_30d),
            Series::new("atm_iv_60d", iv_60d),
            Series::new("atm_iv_90d", iv_90d),
            Series::new("term_spread_30_60", spread_30_60),
            Series::new("term_spread_30_90", spread_30_90),
        ])?;

        // Write to parquet
        let mut file = std::fs::File::create(output_path)?;
        ParquetWriter::new(&mut file).finish(&mut df.clone())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use cs_domain::{repositories::RepositoryError, value_objects::SpotPrice};
    use rust_decimal::Decimal;

    // Mock repositories for testing
    struct MockEquityRepo;
    #[async_trait]
    impl EquityDataRepository for MockEquityRepo {
        async fn get_spot_price(
            &self,
            _symbol: &str,
            _time: DateTime<Utc>,
        ) -> Result<SpotPrice, RepositoryError> {
            Ok(SpotPrice::new(Decimal::new(10000, 2), Utc::now())) // 100.00
        }

        async fn get_bars(
            &self,
            _symbol: &str,
            _date: NaiveDate,
        ) -> Result<DataFrame, RepositoryError> {
            Ok(DataFrame::new(vec![]).unwrap())
        }
    }

    struct MockOptionsRepo;
    #[async_trait]
    impl OptionsDataRepository for MockOptionsRepo {
        async fn get_option_bars(
            &self,
            _symbol: &str,
            _date: NaiveDate,
        ) -> Result<DataFrame, RepositoryError> {
            // Return empty DataFrame with correct schema
            Ok(DataFrame::new(vec![
                Series::new("strike", Vec::<f64>::new()),
                Series::new("expiration", Vec::<i32>::new()),
                Series::new("close", Vec::<f64>::new()),
                Series::new("option_type", Vec::<String>::new()),
            ])
            .unwrap())
        }

        async fn get_available_expirations(
            &self,
            _underlying: &str,
            _as_of_date: NaiveDate,
        ) -> Result<Vec<NaiveDate>, RepositoryError> {
            Ok(vec![])
        }

        async fn get_available_strikes(
            &self,
            _underlying: &str,
            _expiration: NaiveDate,
            _as_of_date: NaiveDate,
        ) -> Result<Vec<cs_domain::value_objects::Strike>, RepositoryError> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_use_case_creation() {
        let equity_repo = MockEquityRepo;
        let options_repo = MockOptionsRepo;
        let use_case = GenerateIvTimeSeriesUseCase::new(equity_repo, options_repo);
        assert_eq!(use_case.market_close.hour, 16);
    }
}
