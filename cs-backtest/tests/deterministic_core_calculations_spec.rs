use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
use cs_backtest::{BacktestConfig, RuleEvaluator};
use cs_domain::testing::EarningsEventBuilder;
use cs_domain::{
    EventRule, LegContext, MarginCalculator, RulesConfig, TradeAccounting, TradeRule, TradeType,
    TradingContext, TradingCostConfig, TradingPeriodSpec, TradingRange,
};
use rust_decimal::Decimal;

#[test]
fn pnl_components_are_deterministic() {
    // Entry debit $2.00, exit credit $2.75 for 1 contract (x100).
    let accounting =
        TradeAccounting::for_debit_trade(Decimal::new(200, 2), Decimal::new(275, 2), 100);

    assert_eq!(accounting.entry_cash_flow, Decimal::new(-200, 0));
    assert_eq!(accounting.exit_cash_flow, Decimal::new(275, 0));
    assert_eq!(accounting.realized_pnl, Decimal::new(75, 0));

    // Add deterministic transaction costs of $10.
    let net = accounting.with_transaction_costs(Decimal::new(1000, 2));
    assert_eq!(net.transaction_costs, Decimal::new(-10, 0));
    assert_eq!(net.realized_pnl, Decimal::new(65, 0));
    assert!((net.return_on_capital - 0.325).abs() < 1e-6);
}

#[test]
fn transaction_costs_are_deterministic() {
    let model = TradingCostConfig::HalfSpread { spread_pct: 0.04 }.build();

    let context = TradingContext::new(
        vec![
            LegContext::long(Decimal::new(250, 2), None),
            LegContext::long(Decimal::new(200, 2), None),
        ],
        "AAPL".to_string(),
        175.0,
        Utc.with_ymd_and_hms(2025, 1, 2, 14, 30, 0).unwrap(),
        TradeType::Straddle,
    );

    // Half-spread per leg:
    // 2.50 * 4% / 2 = 0.05, 2.00 * 4% / 2 = 0.04
    // Entry = (0.05 + 0.04) * 100 = 9.00
    // Round-trip = 18.00
    let entry = model.entry_cost(&context);
    let round_trip = model.round_trip_cost(&context, &context);

    assert_eq!(entry.total, Decimal::new(900, 2));
    assert_eq!(round_trip.total, Decimal::new(1800, 2));
}

#[test]
fn margin_calculations_are_deterministic() {
    let margin = MarginCalculator::reg_t();

    let credit_spread_requirement =
        margin.credit_spread_margin(Decimal::new(150, 2), Decimal::new(500, 2));
    let iron_butterfly_requirement =
        margin.iron_butterfly_margin(Decimal::new(130, 2), Decimal::new(500, 2));

    assert_eq!(credit_spread_requirement, Decimal::new(350, 2));
    assert_eq!(iron_butterfly_requirement, Decimal::new(370, 2));
}

#[test]
fn strategy_signal_rules_are_deterministic() {
    let rules = RulesConfig::default()
        .with_event_rule(EventRule::Symbols {
            include: vec!["AAPL".to_string()],
        })
        .with_event_rule(EventRule::MinMarketCap {
            threshold: 1_000_000_000,
        })
        .with_trade_rule(TradeRule::EntryPriceRange {
            min: Some(0.5),
            max: Some(50.0),
        });

    // Event-level logic (AND): symbol + market cap must pass.
    let passing_event = EarningsEventBuilder::new("AAPL")
        .market_cap(2_000_000_000)
        .build();
    let failing_event = EarningsEventBuilder::new("AAPL")
        .market_cap(250_000_000)
        .build();

    let evaluator = RuleEvaluator::new(rules);
    assert!(evaluator.eval_event_rules(&passing_event));
    assert!(!evaluator.eval_event_rules(&failing_event));

    // Trade-level logic via evaluator API: deterministic boundaries.
    assert!(evaluator.eval_trade_rules(&passing_event, 10.0));
    assert!(!evaluator.eval_trade_rules(&passing_event, 0.25));
    assert!(!evaluator.eval_trade_rules(&passing_event, 75.0));
}

#[test]
fn date_scheduling_windows_are_deterministic() {
    let range = TradingRange::new(
        NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2025, 1, 31).unwrap(),
    );
    assert!(range.contains(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()));
    assert!(range.contains(NaiveDate::from_ymd_opt(2025, 1, 31).unwrap()));
    assert!(!range.contains(NaiveDate::from_ymd_opt(2025, 2, 1).unwrap()));

    let timing = TradingPeriodSpec::CrossEarnings {
        entry_days_before: 1,
        exit_days_after: 1,
        entry_time: NaiveTime::from_hms_opt(9, 35, 0).unwrap(),
        exit_time: NaiveTime::from_hms_opt(15, 55, 0).unwrap(),
    };

    let events = vec![
        EarningsEventBuilder::new("AAPL")
            .earnings_date(NaiveDate::from_ymd_opt(2025, 1, 15).unwrap())
            .build(),
        EarningsEventBuilder::new("MSFT")
            .earnings_date(NaiveDate::from_ymd_opt(2025, 3, 15).unwrap())
            .build(),
    ];

    let tradable = range.discover_tradable_events(&events, &timing);
    assert_eq!(tradable.len(), 1);
    assert_eq!(tradable[0].symbol(), "AAPL");
}

#[test]
fn parameter_validation_logic_is_deterministic() {
    let mut unknown_strategy = BacktestConfig::default();
    unknown_strategy.timing_strategy = Some("NotARealStrategy".to_string());
    let err = unknown_strategy.timing_spec().unwrap_err().to_string();
    assert!(
        err.contains("Unknown timing strategy"),
        "unexpected error: {err}"
    );

    let mut invalid_time = BacktestConfig::default();
    invalid_time.timing.entry_hour = 99;
    let err = invalid_time.timing_spec().unwrap_err().to_string();
    assert!(
        err.contains("Invalid entry_time time"),
        "unexpected error: {err}"
    );
}
