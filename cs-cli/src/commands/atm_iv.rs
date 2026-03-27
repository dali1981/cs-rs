//! ATM IV command handler

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::NaiveDate;
use console::style;

use cs_backtest::{GenerateIvTimeSeriesUseCase, MinuteAlignedIvUseCase};
use cs_domain::value_objects::{AtmIvConfig, HvConfig, IvInterpolationMethod};

#[cfg(feature = "full")]
use cs_domain::infrastructure::{FinqEquityRepository, FinqOptionsRepository};

#[cfg(feature = "demo")]
use cs_domain::infrastructure::{DemoEquityRepository, DemoOptionsRepository};

use crate::args::{AtmIvArgs, GlobalArgs};
use crate::factory::UseCaseFactory;
use super::CommandHandler;

/// ATM IV command handler
pub struct AtmIvCommand {
    args: AtmIvArgs,
    global: GlobalArgs,
}

impl AtmIvCommand {
    /// Create a new ATM IV command handler
    pub fn new(args: AtmIvArgs, global: GlobalArgs) -> Self {
        Self { args, global }
    }
}

#[async_trait]
impl CommandHandler for AtmIvCommand {
    async fn execute(&self) -> Result<()> {
        // Parse dates
        let start_date = NaiveDate::parse_from_str(&self.args.start, "%Y-%m-%d")
            .context("Invalid start date format. Use YYYY-MM-DD")?;
        let end_date = NaiveDate::parse_from_str(&self.args.end, "%Y-%m-%d")
            .context("Invalid end date format. Use YYYY-MM-DD")?;

        // Build config
        let mut config = AtmIvConfig::default();
        if let Some(ref mats) = self.args.maturities {
            config.maturity_targets = mats.clone();
        }
        if let Some(tol) = self.args.tolerance {
            config.maturity_tolerance = tol;
        }
        config.interpolation_method = if self.args.constant_maturity {
            IvInterpolationMethod::ConstantMaturity
        } else {
            IvInterpolationMethod::Rolling
        };
        config.min_dte = self.args.min_dte;

        // Build HV config if requested
        let hv_config = if self.args.with_hv {
            let mut hv_cfg = HvConfig::default();
            if let Some(ref windows) = self.args.hv_windows {
                hv_cfg.windows = windows.clone();
            }
            Some(hv_cfg)
        } else {
            None
        };

        // Determine data directory
        let data_dir = self.global.data_dir
            .clone()
            .or_else(|| std::env::var("FINQ_DATA_DIR").ok().map(std::path::PathBuf::from))
            .context("Data directory not specified. Use --data-dir or set FINQ_DATA_DIR")?;

        println!("{}", style("ATM IV Time Series Generation").bold().cyan());
        println!("Mode: {}", if self.args.eod_pricing { "EOD" } else { "Minute-Aligned (default)" });
        println!("Interpolation: {}", match config.interpolation_method {
            IvInterpolationMethod::Rolling => "Rolling TTE",
            IvInterpolationMethod::ConstantMaturity => "Constant-Maturity (variance interpolation)",
        });
        println!("Symbols: {}", self.args.symbols.join(", "));
        println!("Date range: {} to {}", start_date, end_date);
        println!("Maturities: {:?}", config.maturity_targets);
        println!("Tolerance: {} days", config.maturity_tolerance);
        println!("Min DTE: {}", config.min_dte);
        println!("Output: {:?}", self.args.output);
        println!();

        // Create output directory
        std::fs::create_dir_all(&self.args.output)?;

        // Process each symbol based on mode (minute-aligned is default)
        if !self.args.eod_pricing {
            // Use minute-aligned IV computation (default)
            let use_case = UseCaseFactory::create_minute_aligned_iv(&data_dir)?;

            for symbol in &self.args.symbols {
                println!("{}", style(format!("Processing {}...", symbol)).bold());

                let result = use_case.execute(symbol, start_date, end_date, &config, hv_config.as_ref()).await?;

                println!(
                    "  {} trading days processed, {} successful observations",
                    result.total_days, result.successful_days
                );

                if result.observations.is_empty() {
                    println!("{}", style("  Warning: No observations generated").yellow());
                    continue;
                }

                // Save to parquet
                let output_path = self.args.output.join(format!("atm_iv_{}.parquet", symbol));
                #[cfg(feature = "full")]
                MinuteAlignedIvUseCase::<FinqEquityRepository, FinqOptionsRepository>::save_to_parquet(
                    &result,
                    &output_path,
                )?;
                #[cfg(feature = "demo")]
                MinuteAlignedIvUseCase::<DemoEquityRepository, DemoOptionsRepository>::save_to_parquet(
                    &result,
                    &output_path,
                )?;

                println!(
                    "  {}",
                    style(format!("Saved {} observations to {:?}", result.observations.len(), output_path))
                        .green()
                );

                // Generate plots if requested
                if self.args.plot {
                    println!("  {}", style("Plot generation not yet implemented").yellow());
                }
            }
        } else {
            // Use EOD IV computation (--eod-pricing flag specified)
            let use_case = UseCaseFactory::create_atm_iv(&data_dir)?;

            for symbol in &self.args.symbols {
                println!("{}", style(format!("Processing {}...", symbol)).bold());

                let result = use_case.execute(symbol, start_date, end_date, &config).await?;

                println!(
                    "  {} trading days processed, {} successful observations",
                    result.total_days, result.successful_days
                );

                if result.observations.is_empty() {
                    println!("{}", style("  Warning: No observations generated").yellow());
                    continue;
                }

                // Save to parquet
                let output_path = self.args.output.join(format!("atm_iv_{}.parquet", symbol));
                #[cfg(feature = "full")]
                GenerateIvTimeSeriesUseCase::<FinqEquityRepository, FinqOptionsRepository>::save_to_parquet(
                    &result,
                    &output_path,
                )?;
                #[cfg(feature = "demo")]
                GenerateIvTimeSeriesUseCase::<DemoEquityRepository, DemoOptionsRepository>::save_to_parquet(
                    &result,
                    &output_path,
                )?;

                println!(
                    "  {}",
                    style(format!("Saved {} observations to {:?}", result.observations.len(), output_path))
                        .green()
                );

                // Generate plots if requested
                if self.args.plot {
                    println!("  {}", style("Plot generation not yet implemented").yellow());
                }
            }
        }

        println!();
        println!("{}", style("Done!").bold().green());

        Ok(())
    }
}
