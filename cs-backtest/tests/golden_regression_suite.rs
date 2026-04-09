use chrono::NaiveDate;
use cs_backtest::{
    BacktestPeriod, BacktestResult, RunSummary, SelectionType, SpreadType, StrategyFamily,
    TradeResultMethods,
};
use cs_domain::{CapitalRequirement, HasAccounting, MarginConfig, ReturnBasis};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const GOLDEN_UPDATE_ENV: &str = "CS_GOLDEN_UPDATE";

#[derive(Debug, Deserialize)]
struct GoldenConfig {
    id: String,
    dataset: String,
    strategy: String,
    selection_strategy: String,
    return_basis: String,
    start_date: String,
    end_date: String,
    sessions_processed: usize,
    total_entries: usize,
    total_opportunities: usize,
}

#[derive(Debug, Deserialize)]
struct GoldenDataset {
    id: String,
    trades: Vec<GoldenTradeRecord>,
}

#[derive(Debug, Deserialize)]
struct GoldenTradeRecord {
    entry_cash_flow_cents: i64,
    exit_cash_flow_cents: i64,
    realized_pnl_cents: i64,
    pnl_pct_hundredths: i64,
    capital_required_cents: i64,
    max_loss_cents: Option<i64>,
}

#[derive(Debug, Clone)]
struct GoldenTrade {
    entry_cash_flow: Decimal,
    exit_cash_flow: Decimal,
    realized_pnl: Decimal,
    pnl_pct: Decimal,
    capital_required: Decimal,
    max_loss: Option<Decimal>,
}

impl TradeResultMethods for GoldenTrade {
    fn is_winner(&self) -> bool {
        self.realized_pnl > Decimal::ZERO
    }

    fn pnl(&self) -> Decimal {
        self.realized_pnl
    }

    fn pnl_pct(&self) -> Decimal {
        self.pnl_pct
    }
}

impl HasAccounting for GoldenTrade {
    fn capital_required(&self) -> CapitalRequirement {
        CapitalRequirement::for_debit(self.capital_required)
    }

    fn entry_cash_flow(&self) -> Decimal {
        self.entry_cash_flow
    }

    fn exit_cash_flow(&self) -> Decimal {
        self.exit_cash_flow
    }

    fn realized_pnl(&self) -> Decimal {
        self.realized_pnl
    }

    fn max_loss(&self) -> Option<Decimal> {
        self.max_loss
    }
}

#[derive(Debug, Serialize)]
struct StableSummarySnapshot {
    strategy_family: String,
    strategy: String,
    selection_strategy: String,
    return_basis: String,
    start_date: String,
    end_date: String,
    sessions_processed: usize,
    total_entries: usize,
    total_opportunities: usize,
    trade_count: usize,
    dropped_event_count: usize,
    win_rate_pct: String,
    total_pnl: String,
}

#[derive(Debug, Serialize)]
struct GoldenSnapshot {
    case_id: String,
    trade_count: usize,
    win_rate_pct: String,
    net_pnl: String,
    max_drawdown: String,
    profit_factor: String,
    summary: StableSummarySnapshot,
}

impl GoldenSnapshot {
    fn to_csv(&self) -> String {
        let mut csv = String::from(
            "case_id,trade_count,win_rate_pct,net_pnl,max_drawdown,profit_factor,strategy,selection_strategy,return_basis,start_date,end_date\n",
        );

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{}\n",
            self.case_id,
            self.trade_count,
            self.win_rate_pct,
            self.net_pnl,
            self.max_drawdown,
            self.profit_factor,
            self.summary.strategy,
            self.summary.selection_strategy,
            self.summary.return_basis,
            self.summary.start_date,
            self.summary.end_date,
        ));

        csv
    }
}

#[test]
fn golden_regression_suite_matches_baselines() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let golden_root = repo_root.join("tests/golden");
    let config_dir = golden_root.join("configs");
    let dataset_dir = golden_root.join("datasets");
    let baseline_dir = golden_root.join("baselines");

    let mut config_paths: Vec<PathBuf> = fs::read_dir(&config_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", config_dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    config_paths.sort();

    assert!(
        (2..=4).contains(&config_paths.len()),
        "expected 2-4 canonical golden configs in {}, found {}",
        config_dir.display(),
        config_paths.len()
    );

    let update_baselines = std::env::var_os(GOLDEN_UPDATE_ENV).is_some();

    for config_path in config_paths {
        let config: GoldenConfig = read_json(&config_path);
        let dataset_path = dataset_dir.join(format!("{}.json", config.dataset));
        let dataset: GoldenDataset = read_json(&dataset_path);

        assert_eq!(
            dataset.id, config.id,
            "dataset id {} must match config id {}",
            dataset.id, config.id
        );

        let spread = SpreadType::from_string(&config.strategy);
        let selection_strategy = SelectionType::from_string(&config.selection_strategy);
        let return_basis = parse_return_basis(&config.return_basis);
        let period = BacktestPeriod {
            start_date: parse_date(&config.start_date),
            end_date: parse_date(&config.end_date),
        };

        let trades: Vec<GoldenTrade> = dataset
            .trades
            .into_iter()
            .map(|record| GoldenTrade {
                entry_cash_flow: decimal_from_cents(record.entry_cash_flow_cents),
                exit_cash_flow: decimal_from_cents(record.exit_cash_flow_cents),
                realized_pnl: decimal_from_cents(record.realized_pnl_cents),
                pnl_pct: Decimal::new(record.pnl_pct_hundredths, 2),
                capital_required: decimal_from_cents(record.capital_required_cents),
                max_loss: record.max_loss_cents.map(decimal_from_cents),
            })
            .collect();

        let result = BacktestResult {
            results: trades,
            sessions_processed: config.sessions_processed,
            total_entries: config.total_entries,
            total_opportunities: config.total_opportunities,
            dropped_events: vec![],
            return_basis,
            margin_config: MarginConfig::default(),
        };

        let summary = RunSummary::from_backtest_result(
            StrategyFamily::from_spread(spread),
            spread,
            selection_strategy,
            &period,
            return_basis,
            &result,
        );
        let stats = result.accounting_statistics();

        let snapshot = GoldenSnapshot {
            case_id: config.id.clone(),
            trade_count: summary.trade_count,
            win_rate_pct: summary.win_rate_pct.to_string(),
            net_pnl: summary.total_pnl.to_string(),
            max_drawdown: stats.max_drawdown.to_string(),
            profit_factor: format_f64_metric(stats.profit_factor),
            summary: StableSummarySnapshot {
                strategy_family: summary.strategy_family.to_string(),
                strategy: config.strategy.clone(),
                selection_strategy: config.selection_strategy.clone(),
                return_basis: config.return_basis.clone(),
                start_date: summary.start_date.to_string(),
                end_date: summary.end_date.to_string(),
                sessions_processed: summary.sessions_processed,
                total_entries: summary.total_entries,
                total_opportunities: summary.total_opportunities,
                trade_count: summary.trade_count,
                dropped_event_count: summary.dropped_event_count,
                win_rate_pct: summary.win_rate_pct.to_string(),
                total_pnl: summary.total_pnl.to_string(),
            },
        };

        let actual_json = format!(
            "{}\n",
            serde_json::to_string_pretty(&snapshot)
                .unwrap_or_else(|e| panic!("failed to serialize JSON snapshot for {}: {e}", config.id))
        );
        let actual_csv = snapshot.to_csv();

        let baseline_json_path = baseline_dir.join(format!("{}.summary.json", config.id));
        let baseline_csv_path = baseline_dir.join(format!("{}.summary.csv", config.id));

        if update_baselines {
            fs::write(&baseline_json_path, &actual_json).unwrap_or_else(|e| {
                panic!("failed to write {}: {e}", baseline_json_path.display())
            });
            fs::write(&baseline_csv_path, &actual_csv).unwrap_or_else(|e| {
                panic!("failed to write {}: {e}", baseline_csv_path.display())
            });
            continue;
        }

        let expected_json = fs::read_to_string(&baseline_json_path).unwrap_or_else(|e| {
            panic!(
                "missing baseline {}: {e}\nSet {GOLDEN_UPDATE_ENV}=1 to intentionally regenerate baselines.",
                baseline_json_path.display()
            )
        });
        let expected_csv = fs::read_to_string(&baseline_csv_path).unwrap_or_else(|e| {
            panic!(
                "missing baseline {}: {e}\nSet {GOLDEN_UPDATE_ENV}=1 to intentionally regenerate baselines.",
                baseline_csv_path.display()
            )
        });

        assert_snapshot_equal(
            &expected_json,
            &actual_json,
            &baseline_json_path,
            &config.id,
            "json",
        );
        assert_snapshot_equal(
            &expected_csv,
            &actual_csv,
            &baseline_csv_path,
            &config.id,
            "csv",
        );
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {} as json: {e}", path.display()))
}

fn parse_date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .unwrap_or_else(|e| panic!("invalid date {value}: {e}"))
}

fn parse_return_basis(value: &str) -> ReturnBasis {
    match value {
        "premium" => ReturnBasis::Premium,
        "capital-required" => ReturnBasis::CapitalRequired,
        "max-loss" => ReturnBasis::MaxLoss,
        "bpr-peak" => ReturnBasis::BprPeak,
        "bpr-avg" => ReturnBasis::BprAvg,
        other => panic!("unsupported return_basis '{other}'"),
    }
}

fn decimal_from_cents(cents: i64) -> Decimal {
    Decimal::new(cents, 2)
}

fn format_f64_metric(value: f64) -> String {
    if value.is_infinite() {
        "inf".to_string()
    } else {
        format!("{value:.6}")
    }
}

fn assert_snapshot_equal(
    expected: &str,
    actual: &str,
    baseline_path: &Path,
    case_id: &str,
    kind: &str,
) {
    if expected == actual {
        return;
    }

    panic!(
        "golden {kind} mismatch for case '{case_id}' at {}\n{}\n\nTo intentionally update baselines, set {GOLDEN_UPDATE_ENV}=1 and rerun the test.\nSee docs/golden_baseline_update.md for workflow.",
        baseline_path.display(),
        first_diff(expected, actual)
    );
}

fn first_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let max_len = expected_lines.len().max(actual_lines.len());

    for idx in 0..max_len {
        let e = expected_lines.get(idx).copied();
        let a = actual_lines.get(idx).copied();
        if e == a {
            continue;
        }

        return match (e, a) {
            (Some(ev), Some(av)) => format!(
                "first difference at line {}\n- expected: {}\n+ actual:   {}",
                idx + 1,
                ev,
                av
            ),
            (Some(ev), None) => format!(
                "actual output ended early at line {}\n- expected: {}",
                idx + 1,
                ev
            ),
            (None, Some(av)) => format!(
                "actual output has extra content from line {}\n+ actual: {}",
                idx + 1,
                av
            ),
            (None, None) => "unknown diff".to_string(),
        };
    }

    "content differs but first differing line could not be resolved".to_string()
}
