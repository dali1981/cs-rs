use std::path::{Path, PathBuf};

use chrono::{NaiveDate, Utc};
use cs_backtest::{
    DataSourceConfig, EarningsSourceConfig, RunBacktestCommand, SelectionType, SpreadType,
};
use cs_domain::TradingCostConfig;
use rust_decimal::Decimal;

/// Explicit validation errors for backtest run inputs.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Missing required field: {field}")]
    MissingRequiredField { field: &'static str },
    #[error("Invalid date range: start_date {start_date} must be before end_date {end_date}")]
    InvalidDateRange {
        start_date: NaiveDate,
        end_date: NaiveDate,
    },
    #[error("Future dates are not supported: {field} is {date} but today's date is {today}")]
    FutureDateUnsupported {
        field: &'static str,
        date: NaiveDate,
        today: NaiveDate,
    },
    #[error("Invalid strategy parameter '{field}': {reason}")]
    InvalidStrategyParameter { field: &'static str, reason: String },
    #[error("Invalid cost model configuration: {reason}")]
    InvalidCostModel { reason: String },
    #[error("Missing input data for {field}: {path} ({reason})")]
    MissingInputData {
        field: &'static str,
        path: PathBuf,
        reason: String,
    },
    #[error("Input data is empty for {field}: {path}")]
    EmptyInputData { field: &'static str, path: PathBuf },
}

/// Validate run input before expensive repository wiring and execution.
pub fn validate_run_input(
    command: &RunBacktestCommand,
    data_source: &DataSourceConfig,
    earnings_source: &EarningsSourceConfig,
) -> Result<(), ValidationError> {
    validate_run_input_with_today(
        command,
        data_source,
        earnings_source,
        Utc::now().date_naive(),
    )
}

fn validate_run_input_with_today(
    command: &RunBacktestCommand,
    data_source: &DataSourceConfig,
    earnings_source: &EarningsSourceConfig,
    today: NaiveDate,
) -> Result<(), ValidationError> {
    validate_dates(command, today)?;
    validate_strategy(command)?;
    validate_cost_model(&command.risk.trading_costs)?;
    validate_data_presence(data_source, earnings_source)?;
    Ok(())
}

fn validate_dates(command: &RunBacktestCommand, today: NaiveDate) -> Result<(), ValidationError> {
    let start_date = command.period.start_date;
    let end_date = command.period.end_date;

    if start_date >= end_date {
        return Err(ValidationError::InvalidDateRange {
            start_date,
            end_date,
        });
    }

    if start_date > today {
        return Err(ValidationError::FutureDateUnsupported {
            field: "start_date",
            date: start_date,
            today,
        });
    }

    if end_date > today {
        return Err(ValidationError::FutureDateUnsupported {
            field: "end_date",
            date: end_date,
            today,
        });
    }

    Ok(())
}

fn validate_strategy(command: &RunBacktestCommand) -> Result<(), ValidationError> {
    let target_delta = command.execution.target_delta;
    if !target_delta.is_finite() || !(0.0..=1.0).contains(&target_delta) {
        return Err(ValidationError::InvalidStrategyParameter {
            field: "target_delta",
            reason: "must be within [0.0, 1.0]".to_string(),
        });
    }

    if command.strategy.selection_strategy == SelectionType::DeltaScan {
        let (delta_min, delta_max) = command.execution.delta_range;
        if !delta_min.is_finite()
            || !delta_max.is_finite()
            || !(0.0..=1.0).contains(&delta_min)
            || !(0.0..=1.0).contains(&delta_max)
            || delta_min >= delta_max
        {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "delta_range",
                reason: "must be finite, within [0.0, 1.0], and min < max".to_string(),
            });
        }

        if command.execution.delta_scan_steps == 0 {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "delta_scan_steps",
                reason: "must be greater than 0".to_string(),
            });
        }
    }

    if matches!(
        command.strategy.spread,
        SpreadType::IronButterfly | SpreadType::LongIronButterfly
    ) && (!command.strategy.wing_width.is_finite() || command.strategy.wing_width <= 0.0)
    {
        return Err(ValidationError::InvalidStrategyParameter {
            field: "wing_width",
            reason: "must be greater than 0".to_string(),
        });
    }

    if matches!(
        command.strategy.spread,
        SpreadType::Straddle | SpreadType::ShortStraddle
    ) {
        if command.strategy.straddle_entry_days == 0 {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "straddle_entry_days",
                reason: "must be greater than 0".to_string(),
            });
        }
        if command.strategy.straddle_exit_days == 0 {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "straddle_exit_days",
                reason: "must be greater than 0".to_string(),
            });
        }
        if command.strategy.min_straddle_dte <= 0 {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "min_straddle_dte",
                reason: "must be greater than 0".to_string(),
            });
        }
    }

    if command.strategy.spread == SpreadType::PostEarningsStraddle
        && command.strategy.post_earnings_holding_days == 0
    {
        return Err(ValidationError::InvalidStrategyParameter {
            field: "post_earnings_holding_days",
            reason: "must be greater than 0".to_string(),
        });
    }

    if let Some(min_entry) = command.filters.min_entry_price {
        if min_entry < 0.0 {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "min_entry_price",
                reason: "must be greater than or equal to 0".to_string(),
            });
        }
    }
    if let Some(max_entry) = command.filters.max_entry_price {
        if max_entry < 0.0 {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "max_entry_price",
                reason: "must be greater than or equal to 0".to_string(),
            });
        }
    }
    if let (Some(min_entry), Some(max_entry)) = (
        command.filters.min_entry_price,
        command.filters.max_entry_price,
    ) {
        if min_entry > max_entry {
            return Err(ValidationError::InvalidStrategyParameter {
                field: "entry_price_range",
                reason: "min_entry_price must be <= max_entry_price".to_string(),
            });
        }
    }

    Ok(())
}

fn validate_cost_model(config: &TradingCostConfig) -> Result<(), ValidationError> {
    validate_cost_model_with_depth(config, 0)
}

fn validate_cost_model_with_depth(
    config: &TradingCostConfig,
    depth: usize,
) -> Result<(), ValidationError> {
    const MAX_COMPOSITE_DEPTH: usize = 16;
    if depth > MAX_COMPOSITE_DEPTH {
        return Err(ValidationError::InvalidCostModel {
            reason: format!(
                "composite cost model nesting exceeds max depth ({MAX_COMPOSITE_DEPTH})"
            ),
        });
    }

    match config {
        TradingCostConfig::None => Ok(()),
        TradingCostConfig::Preset { .. } => Ok(()),
        TradingCostConfig::FixedPerLeg { cost_per_leg } => {
            validate_non_negative_decimal(*cost_per_leg, "fixed_per_leg.cost_per_leg")
        }
        TradingCostConfig::Percentage {
            min_cost_per_leg,
            max_cost_per_leg,
            ..
        } => {
            if let Some(min_cost) = min_cost_per_leg {
                validate_non_negative_decimal(*min_cost, "percentage.min_cost_per_leg")?;
            }
            if let Some(max_cost) = max_cost_per_leg {
                validate_non_negative_decimal(*max_cost, "percentage.max_cost_per_leg")?;
            }
            if let (Some(min_cost), Some(max_cost)) = (min_cost_per_leg, max_cost_per_leg) {
                if min_cost > max_cost {
                    return Err(ValidationError::InvalidCostModel {
                        reason:
                            "percentage.min_cost_per_leg must be <= percentage.max_cost_per_leg"
                                .to_string(),
                    });
                }
            }
            Ok(())
        }
        TradingCostConfig::IvBased {
            base_spread_pct,
            iv_multiplier,
            max_spread_pct,
        } => {
            validate_non_negative_f64(*base_spread_pct, "iv_based.base_spread_pct")?;
            validate_non_negative_f64(*iv_multiplier, "iv_based.iv_multiplier")?;
            validate_non_negative_f64(*max_spread_pct, "iv_based.max_spread_pct")?;
            if base_spread_pct > max_spread_pct {
                return Err(ValidationError::InvalidCostModel {
                    reason: "iv_based.base_spread_pct must be <= iv_based.max_spread_pct"
                        .to_string(),
                });
            }
            Ok(())
        }
        TradingCostConfig::HalfSpread { spread_pct } => {
            validate_non_negative_f64(*spread_pct, "half_spread.spread_pct")
        }
        TradingCostConfig::Commission {
            per_contract,
            max_per_leg,
        } => {
            validate_non_negative_decimal(*per_contract, "commission.per_contract")?;
            if let Some(max_per_leg) = max_per_leg {
                validate_non_negative_decimal(*max_per_leg, "commission.max_per_leg")?;
            }
            Ok(())
        }
        TradingCostConfig::Composite {
            slippage,
            commission,
        } => {
            validate_cost_model_with_depth(slippage, depth + 1)?;
            validate_cost_model_with_depth(commission, depth + 1)?;
            Ok(())
        }
    }
}

fn validate_non_negative_f64(value: f64, field: &'static str) -> Result<(), ValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(ValidationError::InvalidCostModel {
            reason: format!("{field} must be finite and >= 0"),
        });
    }
    Ok(())
}

fn validate_non_negative_decimal(
    value: Decimal,
    field: &'static str,
) -> Result<(), ValidationError> {
    if value.is_sign_negative() {
        return Err(ValidationError::InvalidCostModel {
            reason: format!("{field} must be >= 0"),
        });
    }
    Ok(())
}

fn validate_data_presence(
    data_source: &DataSourceConfig,
    earnings_source: &EarningsSourceConfig,
) -> Result<(), ValidationError> {
    let data_dir = data_source.data_dir();
    validate_required_path("data_source.data_dir", data_dir)?;
    validate_existing_and_non_empty("data_source.data_dir", data_dir)?;

    match earnings_source {
        EarningsSourceConfig::File { path } => {
            validate_required_path("earnings_source.file.path", path)?;
            validate_existing_and_non_empty("earnings_source.file.path", path)?;
        }
        EarningsSourceConfig::Provider { dir, .. } => {
            validate_required_path("earnings_source.provider.dir", dir)?;
            validate_existing_and_non_empty("earnings_source.provider.dir", dir)?;
        }
    }

    Ok(())
}

fn validate_required_path(field: &'static str, path: &Path) -> Result<(), ValidationError> {
    if path.as_os_str().is_empty() {
        return Err(ValidationError::MissingRequiredField { field });
    }
    Ok(())
}

fn validate_existing_and_non_empty(
    field: &'static str,
    path: &Path,
) -> Result<(), ValidationError> {
    let metadata = path
        .metadata()
        .map_err(|e| ValidationError::MissingInputData {
            field,
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

    if metadata.is_file() {
        if metadata.len() == 0 {
            return Err(ValidationError::EmptyInputData {
                field,
                path: path.to_path_buf(),
            });
        }
        return Ok(());
    }

    if metadata.is_dir() {
        let mut entries = path
            .read_dir()
            .map_err(|e| ValidationError::MissingInputData {
                field,
                path: path.to_path_buf(),
                reason: e.to_string(),
            })?;

        if entries.next().is_none() {
            return Err(ValidationError::EmptyInputData {
                field,
                path: path.to_path_buf(),
            });
        }
        return Ok(());
    }

    Err(ValidationError::MissingInputData {
        field,
        path: path.to_path_buf(),
        reason: "unsupported file type".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::NaiveDate;
    use cs_backtest::{
        BacktestConfig, DataSourceConfig, EarningsSourceConfig, SelectionType, SpreadType,
    };
    use cs_domain::TradingCostConfig;
    use rust_decimal::Decimal;

    use crate::mapping::map_config_to_command;

    use super::{validate_run_input_with_today, ValidationError};

    fn sample_command() -> cs_backtest::RunBacktestCommand {
        let mut cfg = BacktestConfig::default();
        cfg.start_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        cfg.end_date = NaiveDate::from_ymd_opt(2024, 2, 1).unwrap();
        map_config_to_command(&cfg)
    }

    fn unique_test_dir(suffix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dal154_validation_{suffix}_{ts}"))
    }

    fn make_non_empty_dir(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("marker.txt"), b"x").unwrap();
    }

    fn make_empty_dir(path: &Path) {
        fs::create_dir_all(path).unwrap();
    }

    #[test]
    fn fails_on_missing_required_field() {
        let command = sample_command();
        let data_source = DataSourceConfig::Finq {
            data_dir: PathBuf::new(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: PathBuf::new(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ValidationError::MissingRequiredField { .. }));
    }

    #[test]
    fn fails_on_invalid_date_range() {
        let data_dir = unique_test_dir("date_range_data");
        let earnings_dir = unique_test_dir("date_range_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let mut command = sample_command();
        command.period.start_date = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        command.period.end_date = NaiveDate::from_ymd_opt(2024, 2, 1).unwrap();
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ValidationError::InvalidDateRange { .. }));

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn fails_on_future_date() {
        let data_dir = unique_test_dir("future_date_data");
        let earnings_dir = unique_test_dir("future_date_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let mut command = sample_command();
        command.period.end_date = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ValidationError::FutureDateUnsupported { .. }));

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn fails_on_missing_data_file() {
        let data_dir = unique_test_dir("missing_file_data");
        make_non_empty_dir(&data_dir);
        let missing_file = unique_test_dir("missing_earnings_file").join("earnings.parquet");

        let command = sample_command();
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::File {
            path: missing_file.clone(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ValidationError::MissingInputData { .. }));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn fails_on_empty_dataset() {
        let data_dir = unique_test_dir("empty_dataset_data");
        let earnings_dir = unique_test_dir("empty_dataset_earnings");
        make_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let command = sample_command();
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ValidationError::EmptyInputData { .. }));

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn passes_valid_input() {
        let data_dir = unique_test_dir("valid_data");
        let earnings_dir = unique_test_dir("valid_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let command = sample_command();
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let result = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        );

        assert!(result.is_ok());
        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn allows_unused_strategy_fields_for_calendar_spread() {
        let data_dir = unique_test_dir("calendar_spread_data");
        let earnings_dir = unique_test_dir("calendar_spread_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let mut command = sample_command();
        command.strategy.spread = SpreadType::Calendar;
        command.strategy.wing_width = 0.0;
        command.strategy.straddle_entry_days = 0;
        command.strategy.straddle_exit_days = 0;
        command.strategy.min_straddle_dte = 0;
        command.execution.delta_range = (0.9, 0.1);
        command.strategy.selection_strategy = SelectionType::ATM;

        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let result = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        );

        assert!(result.is_ok());
        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn fails_on_invalid_wing_width_for_iron_butterfly() {
        let data_dir = unique_test_dir("ibf_data");
        let earnings_dir = unique_test_dir("ibf_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let mut command = sample_command();
        command.strategy.spread = SpreadType::IronButterfly;
        command.strategy.wing_width = 0.0;
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ValidationError::InvalidStrategyParameter {
                field: "wing_width",
                ..
            }
        ));

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn fails_on_invalid_delta_range_for_delta_scan() {
        let data_dir = unique_test_dir("delta_scan_data");
        let earnings_dir = unique_test_dir("delta_scan_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let mut command = sample_command();
        command.strategy.selection_strategy = SelectionType::DeltaScan;
        command.execution.delta_range = (0.8, 0.2);
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ValidationError::InvalidStrategyParameter {
                field: "delta_range",
                ..
            }
        ));

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }

    #[test]
    fn fails_on_invalid_cost_model_parameters() {
        let data_dir = unique_test_dir("cost_model_data");
        let earnings_dir = unique_test_dir("cost_model_earnings");
        make_non_empty_dir(&data_dir);
        make_non_empty_dir(&earnings_dir);

        let mut command = sample_command();
        command.risk.trading_costs = TradingCostConfig::Percentage {
            slippage_bps: 10,
            min_cost_per_leg: Some(Decimal::new(5, 0)),
            max_cost_per_leg: Some(Decimal::new(1, 0)),
        };
        let data_source = DataSourceConfig::Finq {
            data_dir: data_dir.clone(),
        };
        let earnings_source = EarningsSourceConfig::Provider {
            dir: earnings_dir.clone(),
            source: Default::default(),
        };

        let err = validate_run_input_with_today(
            &command,
            &data_source,
            &earnings_source,
            NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ValidationError::InvalidCostModel { .. }));

        let _ = fs::remove_dir_all(data_dir);
        let _ = fs::remove_dir_all(earnings_dir);
    }
}
