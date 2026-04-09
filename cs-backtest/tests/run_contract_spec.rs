use chrono::NaiveDate;
use cs_backtest::{
    BacktestPeriod, BacktestResult, RunSummary, SelectionType, SpreadType, StrategyFamily,
    TradeResultMethods,
};
use cs_domain::{MarginConfig, ReturnBasis};
use rust_decimal::Decimal;
use std::path::Path;

#[derive(Debug, Clone)]
struct DummyTrade {
    pnl: Decimal,
    pnl_pct: Decimal,
}

impl TradeResultMethods for DummyTrade {
    fn is_winner(&self) -> bool {
        self.pnl > Decimal::ZERO
    }

    fn pnl(&self) -> Decimal {
        self.pnl
    }

    fn pnl_pct(&self) -> Decimal {
        self.pnl_pct
    }
}

fn sample_result() -> BacktestResult<DummyTrade> {
    BacktestResult {
        results: vec![
            DummyTrade {
                pnl: Decimal::new(1250, 2),
                pnl_pct: Decimal::new(500, 2),
            },
            DummyTrade {
                pnl: Decimal::new(-300, 2),
                pnl_pct: Decimal::new(-100, 2),
            },
        ],
        sessions_processed: 1,
        total_entries: 2,
        total_opportunities: 3,
        dropped_events: vec![],
        return_basis: ReturnBasis::Premium,
        margin_config: MarginConfig::default(),
    }
}

#[test]
fn strategy_family_maps_supported_spreads() {
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::Calendar),
        StrategyFamily::CalendarSpread
    );
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::IronButterfly),
        StrategyFamily::IronButterfly
    );
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::LongIronButterfly),
        StrategyFamily::IronButterfly
    );
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::Straddle),
        StrategyFamily::Straddle
    );
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::ShortStraddle),
        StrategyFamily::Straddle
    );
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::CalendarStraddle),
        StrategyFamily::CalendarStraddle
    );
    assert_eq!(
        StrategyFamily::from_spread(SpreadType::PostEarningsStraddle),
        StrategyFamily::PostEarningsStraddle
    );
}

#[test]
fn run_summary_captures_required_fields() {
    let period = BacktestPeriod {
        start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        end_date: NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
    };
    let result = sample_result();

    let summary = RunSummary::from_backtest_result(
        StrategyFamily::CalendarSpread,
        SpreadType::Calendar,
        SelectionType::ATM,
        &period,
        ReturnBasis::Premium,
        &result,
    );

    assert_eq!(summary.start_date, period.start_date);
    assert_eq!(summary.end_date, period.end_date);
    assert_eq!(summary.strategy, SpreadType::Calendar);
    assert_eq!(summary.selection_strategy, SelectionType::ATM);
    assert_eq!(summary.total_entries, 2);
    assert_eq!(summary.total_opportunities, 3);
    assert_eq!(summary.trade_count, 2);
    assert_eq!(summary.total_pnl, Decimal::new(950, 2));
    assert_eq!(summary.win_rate_pct, Decimal::from(50u64));
    assert_eq!(summary.return_basis, ReturnBasis::Premium);
}

#[test]
fn spec_docs_exist_with_required_sections() {
    let docs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
    let architecture = docs_dir.join("ARCHITECTURE_AND_RUN_SPEC.md");
    let contract = docs_dir.join("run_contract.md");

    assert!(architecture.exists(), "missing {}", architecture.display());
    assert!(contract.exists(), "missing {}", contract.display());

    let architecture_text = std::fs::read_to_string(&architecture).unwrap();
    let contract_text = std::fs::read_to_string(&contract).unwrap();

    assert!(architecture_text.contains("Canonical Entrypoint"));
    assert!(architecture_text.contains("Run Lifecycle"));
    assert!(architecture_text.contains("Invariants"));
    assert!(contract_text.contains("RunInput"));
    assert!(contract_text.contains("RunOutput"));
    assert!(contract_text.contains("RunSummary"));
}
