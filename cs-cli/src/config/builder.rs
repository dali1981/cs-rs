//! BacktestConfig builder from CLI args

use anyhow::{Context, Result};
use std::path::PathBuf;
use chrono::NaiveDate;

use cs_backtest::BacktestConfig;
use crate::args::{BacktestArgs, GlobalArgs};
use crate::config::load_config;
use crate::cli_args::CliOverrides;

/// Builder for BacktestConfig from CLI args
pub struct BacktestConfigBuilder {
    config_files: Vec<PathBuf>,
    global: Option<GlobalArgs>,
    args: Option<BacktestArgs>,
}

impl BacktestConfigBuilder {
    /// Create builder with defaults
    pub fn new() -> Self {
        Self {
            config_files: Vec::new(),
            global: None,
            args: None,
        }
    }

    /// Create builder from backtest args
    pub fn from_args(args: &BacktestArgs) -> Self {
        Self {
            config_files: Vec::new(),
            global: None,
            args: Some(args.clone()),
        }
    }

    /// Apply global args
    pub fn with_global(mut self, global: &GlobalArgs) -> Self {
        self.global = Some(global.clone());
        self
    }

    /// Merge TOML config files (lower priority than CLI)
    pub fn with_config_files(mut self, files: &[PathBuf]) -> Result<Self> {
        self.config_files = files.to_vec();
        Ok(self)
    }

    /// Build and validate the config
    pub fn build(self) -> Result<BacktestConfig> {
        // Build CliOverrides from args
        let cli_overrides = self.build_cli_overrides();

        // Load config using figment (merges defaults, system config, strategy configs, CLI)
        let app_config = load_config(&self.config_files, cli_overrides)
            .context("Failed to load configuration")?;

        // Convert AppConfig to BacktestConfig
        let mut config = app_config.to_backtest_config();

        // Override data_dir if provided via global args (highest priority)
        if let Some(ref global) = self.global {
            if let Some(ref data_dir) = global.data_dir {
                config.data_dir = data_dir.clone();
            }
        }

        // Parse and set dates from args (required)
        if let Some(ref args) = self.args {
            config.start_date = Self::parse_date(&args.start)?;
            config.end_date = Self::parse_date(&args.end)?;

            // Set earnings_file if provided (takes precedence over earnings_dir)
            if let Some(ref earnings_file) = args.earnings_file {
                config.earnings_file = Some(earnings_file.clone());
            }

            // Set earnings_dir if provided
            if let Some(ref earnings_dir) = args.earnings_dir {
                config.earnings_dir = earnings_dir.clone();
            }
        }

        // Validate required fields
        if config.data_dir.as_os_str().is_empty() {
            anyhow::bail!("Data directory is required. Set --data-dir or FINQ_DATA_DIR");
        }

        Ok(config)
    }

    /// Parse date string to NaiveDate
    fn parse_date(s: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("Invalid date format: {}. Use YYYY-MM-DD", s))
    }

    /// Build CliOverrides from BacktestArgs and GlobalArgs
    fn build_cli_overrides(&self) -> CliOverrides {
        use crate::cli_args::{CliPaths, CliTiming, CliSelection, CliStrategy, CliHedging, CliAttribution};
        use crate::parsing::parse_time;

        let mut overrides = CliOverrides::default();

        // Apply global args - paths
        if let Some(ref global) = self.global {
            if let Some(ref data_dir) = global.data_dir {
                overrides.paths = Some(CliPaths {
                    data_dir: Some(data_dir.clone()),
                    earnings_dir: None,
                });
            }
        }

        // Apply backtest args
        if let Some(ref args) = self.args {
            // Strategy
            let spread_str = format!("{}", args.strategy.spread);
            let selection_str = format!("{}", args.strategy.selection);

            overrides.strategy = Some(CliStrategy {
                spread_type: Some(spread_str),
                selection_type: Some(selection_str),
                target_delta: args.selection.target_delta,
                ..Default::default()
            });

            // Timing - parse time strings to hour/minute and populate generic timing fields
            let mut timing = CliTiming::default();
            if let Some(ref entry_time) = args.timing.entry_time {
                if let Ok((hour, minute)) = parse_time(Some(entry_time.clone())) {
                    timing.entry_hour = hour;
                    timing.entry_minute = minute;
                }
            }
            if let Some(ref exit_time) = args.timing.exit_time {
                if let Ok((hour, minute)) = parse_time(Some(exit_time.clone())) {
                    timing.exit_hour = hour;
                    timing.exit_minute = minute;
                }
            }
            // Populate generic timing fields from CLI args
            timing.timing_strategy = args.timing.timing_strategy.clone();
            timing.entry_days_before = args.timing.entry_days_before;
            timing.exit_days_before = args.timing.exit_days_before;
            timing.entry_offset = args.timing.entry_offset;
            timing.holding_days = args.timing.holding_days;
            timing.exit_days_after = args.timing.exit_days_after;

            if timing.entry_hour.is_some()
                || timing.exit_hour.is_some()
                || timing.timing_strategy.is_some()
                || timing.entry_days_before.is_some()
                || timing.exit_days_before.is_some()
                || timing.entry_offset.is_some()
                || timing.holding_days.is_some()
                || timing.exit_days_after.is_some() {
                overrides.timing = Some(timing);
            }

            // Selection criteria
            overrides.selection = Some(CliSelection {
                min_short_dte: args.selection.min_short_dte,
                max_short_dte: args.selection.max_short_dte,
                min_long_dte: args.selection.min_long_dte,
                max_long_dte: args.selection.max_long_dte,
                target_delta: args.selection.target_delta,
                min_iv_ratio: args.selection.min_iv_ratio,
            });

            // Symbols and filters
            if let Some(ref symbols) = args.symbols {
                if !symbols.is_empty() {
                    overrides.symbols = Some(symbols.clone());
                }
            }
            if args.no_parallel {
                overrides.parallel = Some(false);
            }

            // Hedging
            if args.hedging.hedge {
                overrides.hedging = Some(CliHedging {
                    enabled: Some(true),
                    strategy: Some(args.hedging.hedge_strategy.clone()),
                    delta_threshold: Some(args.hedging.delta_threshold),
                    interval_hours: Some(args.hedging.hedge_interval_hours),
                    max_rehedges: args.hedging.max_rehedges,
                    delta_mode: Some(args.hedging.hedge_delta_mode.clone()),
                    hv_window: Some(args.hedging.hv_window),
                    cost_per_share: Some(args.hedging.hedge_cost_per_share),
                    track_realized_vol: None,
                });
            }

            // Attribution
            if args.attribution.attribution {
                overrides.attribution = Some(CliAttribution {
                    enabled: Some(true),
                    vol_source: Some(args.attribution.attribution_vol_source.clone()),
                    snapshot_times: Some(args.attribution.attribution_snapshots.clone()),
                });
            }
        }

        overrides
    }
}

impl Default for BacktestConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
