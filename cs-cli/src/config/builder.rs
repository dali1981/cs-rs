//! BacktestConfig builder from CLI args

use anyhow::{Context, Result};
use chrono::NaiveDate;
use std::path::PathBuf;

use super::validation::validate_run_input;
use crate::args::{BacktestArgs, GlobalArgs};
use crate::cli_args::CliOverrides;
use crate::config::load_config;
use crate::mapping::map_config_to_command;
use cs_backtest::{BacktestConfig, DataSourceConfig, EarningsSourceConfig, RunBacktestCommand};

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

    /// Build and split business intent from infrastructure configuration.
    ///
    /// Returns `(command, data_source, earnings_source)` separately so the
    /// factory can wire each independently (ADR-0003).
    pub fn build(self) -> Result<(RunBacktestCommand, DataSourceConfig, EarningsSourceConfig)> {
        let config = self.build_raw_config()?;
        config
            .timing_spec()
            .map_err(|e| anyhow::anyhow!("Invalid timing strategy configuration: {e}"))?;
        let data_source = config.data_source.clone();
        let earnings_source = config.earnings_source.clone();
        let command = map_config_to_command(&config);
        validate_run_input(&command, &data_source, &earnings_source)
            .context("Backtest input validation failed")?;
        Ok((command, data_source, earnings_source))
    }

    /// Build the intermediate `BacktestConfig` (TOML DTO). Used internally and
    /// retained for backward-compatible callers that need the raw config (e.g.
    /// display logic that reads `config.data_source` before the use case runs).
    pub fn build_raw_config(self) -> Result<BacktestConfig> {
        // Build CliOverrides from args
        let cli_overrides = self.build_cli_overrides()?;

        // Load config using figment (merges defaults, system config, strategy configs, CLI)
        let app_config = load_config(&self.config_files, cli_overrides)
            .context("Failed to load configuration")?;

        // Convert AppConfig to BacktestConfig
        let mut config = app_config.to_backtest_config();

        // Handle data source configuration from CLI args
        if let Some(ref args) = self.args {
            // Build DataSourceConfig based on --data-source flag
            match args.data_source.to_lowercase().as_str() {
                "ib" => {
                    let ib_data_dir = args
                        .ib_data_dir
                        .clone()
                        .or_else(|| {
                            std::env::var("IB_DATA_DIR")
                                .ok()
                                .map(std::path::PathBuf::from)
                        })
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "IB data directory required when using --data-source ib. \
                             Set --ib-data-dir or IB_DATA_DIR environment variable."
                            )
                        })?;
                    config.data_source = DataSourceConfig::Ib {
                        data_dir: ib_data_dir,
                    };
                }
                "finq" => {
                    // Default to Finq - use global data_dir or FINQ_DATA_DIR
                    let finq_data_dir = if let Some(ref global) = self.global {
                        global.data_dir.clone()
                    } else {
                        None
                    }
                    .or_else(|| {
                        std::env::var("FINQ_DATA_DIR")
                            .ok()
                            .map(std::path::PathBuf::from)
                    })
                    .unwrap_or_else(|| {
                        dirs::home_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("polygon/data")
                    });
                    config.data_source = DataSourceConfig::Finq {
                        data_dir: finq_data_dir,
                    };
                }
                unknown => {
                    return Err(anyhow::anyhow!(
                        "Unsupported data source '{}'. Valid values: finq, ib",
                        unknown
                    ));
                }
            }
        }

        // Parse and set dates from args (required)
        if let Some(ref args) = self.args {
            config.start_date = Self::parse_date(&args.start)?;
            config.end_date = Self::parse_date(&args.end)?;

            // Build unified EarningsSourceConfig from CLI args
            // Priority: file > provider
            use cs_backtest::EarningsProvider;

            config.earnings_source = if let Some(ref earnings_file) = args.earnings_file {
                EarningsSourceConfig::file(earnings_file.clone())
            } else {
                let dir = args
                    .earnings_dir
                    .clone()
                    .or_else(|| std::env::var("EARNINGS_DATA_DIR").ok().map(PathBuf::from))
                    .unwrap_or_else(|| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("."))
                            .join("trading_project/nasdaq_earnings/data")
                    });
                let source = EarningsProvider::from_str(&args.earnings_source)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                EarningsSourceConfig::provider(source, dir)
            };

            // Apply CLI rules (override TOML rules if any CLI rule flags are set)
            if args.rules.has_rules() {
                config.rules = Self::build_file_rules_from_cli(&args.rules);
            }
        }

        Ok(config)
    }

    /// Build FileRulesConfig from CLI args
    fn build_file_rules_from_cli(args: &crate::args::RulesArgs) -> cs_domain::FileRulesConfig {
        use cs_domain::{FileRulesConfig, MarketRule};

        let mut market_rules = Vec::new();

        // IV Slope rule
        if args.entry_iv_slope {
            market_rules.push(MarketRule::IvSlope {
                short_dte: args.iv_slope_short_dte.unwrap_or(7),
                long_dte: args.iv_slope_long_dte.unwrap_or(20),
                threshold_pp: args.iv_slope_threshold.unwrap_or(0.05),
            });
        }

        // IV vs HV rule
        if args.entry_iv_vs_hv {
            market_rules.push(MarketRule::IvVsHv {
                hv_window_days: args.iv_hv_window.unwrap_or(20),
                min_ratio: args.iv_hv_ratio.unwrap_or(1.0),
            });
        }

        FileRulesConfig {
            event: None, // CLI doesn't override event rules
            market: if market_rules.is_empty() {
                None
            } else {
                Some(market_rules)
            },
            trade: None, // CLI doesn't override trade rules
        }
    }

    /// Parse date string to NaiveDate
    fn parse_date(s: &str) -> Result<NaiveDate> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .with_context(|| format!("Invalid date format: {}. Use YYYY-MM-DD", s))
    }

    /// Build CliOverrides from BacktestArgs and GlobalArgs
    fn build_cli_overrides(&self) -> Result<CliOverrides> {
        use crate::cli_args::{
            CliAttribution, CliHedging, CliMetrics, CliPaths, CliPricing, CliSelection,
            CliStrategy, CliTiming,
        };
        use crate::parsing::{parse_delta_range, parse_time};

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
            // Strategy - only add if explicitly provided
            let mut strategy = CliStrategy::default();
            let mut has_strategy_override = false;

            if let Some(spread) = args.strategy.spread {
                strategy.spread_type = Some(format!("{}", spread));
                has_strategy_override = true;
            }

            if let Some(selection) = args.strategy.selection {
                strategy.selection_type = Some(format!("{}", selection));
                has_strategy_override = true;
            }

            if let Some(delta) = args.selection.target_delta {
                strategy.target_delta = Some(delta);
                has_strategy_override = true;
            }

            if let Some(delta_range) = parse_delta_range(args.strategy.delta_range.clone())? {
                strategy.delta_range = Some(delta_range);
                has_strategy_override = true;
            }

            if let Some(delta_scan_steps) = args.strategy.delta_scan_steps {
                strategy.delta_scan_steps = Some(delta_scan_steps);
                has_strategy_override = true;
            }

            if let Some(wing_width) = args.strategy.wing_width {
                strategy.wing_width = Some(wing_width);
                has_strategy_override = true;
            }

            if let Some(straddle_entry_days) = args.strategy.straddle_entry_days {
                strategy.straddle_entry_days = Some(straddle_entry_days);
                has_strategy_override = true;
            }

            if let Some(straddle_exit_days) = args.strategy.straddle_exit_days {
                strategy.straddle_exit_days = Some(straddle_exit_days);
                has_strategy_override = true;
            }

            if let Some(min_straddle_dte) = args.strategy.min_straddle_dte {
                strategy.min_straddle_dte = Some(min_straddle_dte);
                has_strategy_override = true;
            }

            if let Some(min_entry_price) = args.strategy.min_entry_price {
                strategy.min_entry_price = Some(min_entry_price);
                has_strategy_override = true;
            }

            if let Some(max_entry_price) = args.strategy.max_entry_price {
                strategy.max_entry_price = Some(max_entry_price);
                has_strategy_override = true;
            }

            if let Some(post_earnings_holding_days) = args.strategy.post_earnings_holding_days {
                strategy.post_earnings_holding_days = Some(post_earnings_holding_days);
                has_strategy_override = true;
            }

            // Only set overrides.strategy if user provided at least one strategy field
            if has_strategy_override {
                overrides.strategy = Some(strategy);
            }

            // Timing - parse time strings to hour/minute and populate generic timing fields
            let mut timing = CliTiming::default();
            if let Some(ref entry_time) = args.timing.entry_time {
                let (hour, minute) = parse_time(Some(entry_time.clone()))?;
                timing.entry_hour = hour;
                timing.entry_minute = minute;
            }
            if let Some(ref exit_time) = args.timing.exit_time {
                let (hour, minute) = parse_time(Some(exit_time.clone()))?;
                timing.exit_hour = hour;
                timing.exit_minute = minute;
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
                || timing.exit_days_after.is_some()
            {
                overrides.timing = Some(timing);
            }

            // Metrics
            let mut metrics = CliMetrics::default();
            let mut has_metrics_override = false;
            if let Some(ref return_basis) = args.metrics.return_basis {
                metrics.return_basis = Some(return_basis.clone());
                has_metrics_override = true;
            }
            if has_metrics_override {
                overrides.metrics = Some(metrics);
            }

            // Pricing
            let mut pricing = CliPricing::default();
            let mut has_pricing_override = false;
            if let Some(ref pricing_model) = args.pricing_model {
                pricing.model = Some(pricing_model.clone());
                has_pricing_override = true;
            }
            if let Some(ref vol_model) = args.vol_model {
                pricing.vol_model = Some(vol_model.clone());
                has_pricing_override = true;
            }
            if has_pricing_override {
                overrides.pricing = Some(pricing);
            }

            // Strike matching
            if let Some(ref strike_match_mode) = args.strike_match_mode {
                overrides.strike_match_mode = Some(strike_match_mode.clone());
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
                    strategy: args.hedging.hedge_strategy.clone(),
                    delta_threshold: args.hedging.delta_threshold,
                    interval_hours: args.hedging.hedge_interval_hours,
                    max_rehedges: args.hedging.max_rehedges,
                    delta_mode: args.hedging.hedge_delta_mode.clone(),
                    hv_window: args.hedging.hv_window,
                    cost_per_share: args.hedging.hedge_cost_per_share,
                    track_realized_vol: None,
                });
            }

            // Attribution
            if args.attribution.attribution {
                overrides.attribution = Some(CliAttribution {
                    enabled: Some(true),
                    vol_source: args.attribution.attribution_vol_source.clone(),
                    snapshot_times: args.attribution.attribution_snapshots.clone(),
                });
            }

            // Rules - only populate if any rule flags are set
            if args.rules.has_rules() {
                overrides.rules = Some(crate::cli_args::CliRules {
                    iv_slope_enabled: args.rules.entry_iv_slope,
                    iv_slope_short_dte: args.rules.iv_slope_short_dte,
                    iv_slope_long_dte: args.rules.iv_slope_long_dte,
                    iv_slope_threshold: args.rules.iv_slope_threshold,
                    iv_vs_hv_enabled: args.rules.entry_iv_vs_hv,
                    iv_hv_window: args.rules.iv_hv_window,
                    iv_hv_ratio: args.rules.iv_hv_ratio,
                });
            }
        }

        Ok(overrides)
    }
}

impl Default for BacktestConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::args::{
        AttributionArgs, BacktestArgs, GlobalArgs, HedgingArgs, MetricsArgs, RulesArgs,
        SelectionArgs, StrategyArgs, TimingArgs,
    };
    use crate::config::BacktestConfigBuilder;

    fn unique_test_path(name: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dal154_builder_{name}_{ts}"))
    }

    fn minimal_backtest_args(conf: Vec<PathBuf>) -> BacktestArgs {
        BacktestArgs {
            conf,
            data_source: "finq".to_string(),
            ib_data_dir: None,
            earnings_source: "tradingview".to_string(),
            earnings_dir: None,
            earnings_file: None,
            start: "2024-01-01".to_string(),
            end: "2024-02-01".to_string(),
            symbols: None,
            output: None,
            pricing_model: None,
            vol_model: None,
            strike_match_mode: None,
            no_parallel: false,
            timing: TimingArgs {
                entry_time: None,
                exit_time: None,
                timing_strategy: None,
                entry_days_before: None,
                exit_days_before: None,
                entry_offset: None,
                holding_days: None,
                exit_days_after: None,
            },
            selection: SelectionArgs {
                min_short_dte: None,
                max_short_dte: None,
                min_long_dte: None,
                max_long_dte: None,
                target_delta: None,
                min_iv_ratio: None,
                min_market_cap: None,
                min_notional: None,
                max_entry_iv: None,
            },
            strategy: StrategyArgs {
                spread: None,
                selection: None,
                option_type: None,
                delta_range: None,
                delta_scan_steps: None,
                wing_width: None,
                straddle_entry_days: None,
                straddle_exit_days: None,
                min_straddle_dte: None,
                min_entry_price: None,
                max_entry_price: None,
                post_earnings_holding_days: None,
                roll_strategy: None,
                roll_day: None,
            },
            hedging: HedgingArgs {
                hedge: false,
                hedge_strategy: None,
                hedge_interval_hours: None,
                delta_threshold: None,
                max_rehedges: None,
                hedge_cost_per_share: None,
                hedge_delta_mode: None,
                hv_window: None,
                track_realized_vol: false,
            },
            attribution: AttributionArgs {
                attribution: false,
                attribution_vol_source: None,
                attribution_snapshots: None,
            },
            rules: RulesArgs::default(),
            metrics: MetricsArgs::default(),
        }
    }

    #[test]
    fn build_raw_config_rejects_unknown_cost_model() {
        let conf_path = unique_test_path("unknown_cost_model.toml");
        fs::write(
            &conf_path,
            r#"
[trading_costs]
model = "not_a_real_model"
"#,
        )
        .unwrap();

        let args = minimal_backtest_args(vec![conf_path.clone()]);
        let global = GlobalArgs {
            data_dir: None,
            verbose: false,
        };

        let result = BacktestConfigBuilder::from_args(&args)
            .with_global(&global)
            .with_config_files(&args.conf)
            .unwrap()
            .build_raw_config();

        assert!(result.is_err(), "expected unknown cost model to fail");
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("unknown variant")
                || err_msg.contains("trading_costs")
                || err_msg.contains("model"),
            "unexpected error message: {err_msg}"
        );

        let _ = fs::remove_file(conf_path);
    }

    #[test]
    fn build_rejects_unknown_timing_strategy() {
        let mut args = minimal_backtest_args(Vec::new());
        args.timing.timing_strategy = Some("NotARealStrategy".to_string());

        let global = GlobalArgs {
            data_dir: None,
            verbose: false,
        };

        let result = BacktestConfigBuilder::from_args(&args)
            .with_global(&global)
            .with_config_files(&args.conf)
            .unwrap()
            .build();

        assert!(result.is_err(), "expected unknown timing strategy to fail");
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(
            err_msg.contains("Unknown timing strategy"),
            "unexpected error message: {err_msg}"
        );
    }
}
