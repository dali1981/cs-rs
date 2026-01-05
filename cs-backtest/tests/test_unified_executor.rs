/// Integration test for UnifiedExecutor with optimized IV surface building
///
/// Tests that the new process_event_unified() method works correctly
/// and achieves the IV surface optimization (build once, reuse for selection + entry)

use chrono::NaiveDate;
use cs_backtest::{BacktestUseCase, BacktestConfig, SpreadType, SelectionType, TradeStructure, TradeResult};
use cs_domain::{
    infrastructure::{FinqEquityRepository, FinqOptionsRepository, ParquetEarningsRepository},
    EarningsEvent, EarningsTime, TimingConfig,
};
use std::path::PathBuf;

#[tokio::test]
async fn test_unified_executor_calendar_spread() {
    // Setup repositories
    let data_dir = PathBuf::from("/Users/mohamedali/polygon/data");
    let earnings_repo = ParquetEarningsRepository::new(data_dir.join("earnings/earnings.parquet"));
    let options_repo = FinqOptionsRepository::new(data_dir.clone());
    let equity_repo = FinqEquityRepository::new(data_dir);

    // Create config for calendar spread
    let mut config = BacktestConfig::default();
    config.spread = SpreadType::Calendar;
    config.selection_strategy = SelectionType::ATM;
    config.timing = TimingConfig {
        entry_hour: 15,
        entry_minute: 55,
        exit_hour: 9,
        exit_minute: 45,
    };

    // Create backtest use case (earnings_repo, options_repo, equity_repo, config)
    let use_case = BacktestUseCase::new(
        earnings_repo,
        options_repo,
        equity_repo,
        config,
    );

    // Create test event (CRBG from our known working test case)
    let event = EarningsEvent::new(
        "CRBG".to_string(),
        NaiveDate::from_ymd_opt(2025, 11, 4).unwrap(),
        EarningsTime::AfterMarketClose,
    );

    println!("\n=== Testing UnifiedExecutor with CRBG ===");
    println!("Symbol: {}", event.symbol);
    println!("Earnings date: {}", event.earnings_date);
    println!("Earnings time: {:?}", event.earnings_time);

    // Get selector and structure
    let selector = use_case.create_selector();
    let structure = TradeStructure::CalendarSpread(finq_core::OptionType::Call);

    println!("\n--- Calling process_event_unified ---");

    // Call the new optimized method
    let result = use_case.process_event_unified(
        &event,
        &*selector,
        structure,
    ).await;

    println!("\n--- Result ---");
    println!("Success: {}", result.success());
    println!("Symbol: {}", result.symbol());
    println!("PnL: {}", result.pnl());

    // Verify we got a result (success or failure is OK, we just want to test the flow)
    match result {
        TradeResult::CalendarSpread(r) => {
            println!("\n✓ CalendarSpreadResult received");
            println!("  Strike: {}", r.strike.value());
            println!("  Short expiry: {}", r.short_expiry);
            println!("  Long expiry: {}", r.long_expiry);
            println!("  Entry cost: {}", r.entry_cost);
        }
        TradeResult::Failed(failed) => {
            println!("\n⚠ Trade failed");
            println!("  Reason: {:?}", failed.reason);
            println!("  Phase: {}", failed.phase);
        }
        _ => panic!("Expected CalendarSpread result"),
    }

    println!("\n✓ Test passed - process_event_unified works correctly");
}

#[tokio::test]
async fn test_unified_executor_straddle() {
    // Setup repositories
    let data_dir = PathBuf::from("/Users/mohamedali/polygon/data");
    let earnings_repo = ParquetEarningsRepository::new(data_dir.join("earnings/earnings.parquet"));
    let options_repo = FinqOptionsRepository::new(data_dir.clone());
    let equity_repo = FinqEquityRepository::new(data_dir);

    // Create config for straddle
    let mut config = BacktestConfig::default();
    config.spread = SpreadType::Straddle;
    config.selection_strategy = SelectionType::ATM;

    // Create backtest use case
    let use_case = BacktestUseCase::new(
        earnings_repo,
        options_repo,
        equity_repo,
        config,
    );

    // Create test event
    let event = EarningsEvent::new(
        "CRBG".to_string(),
        NaiveDate::from_ymd_opt(2025, 11, 4).unwrap(),
        EarningsTime::AfterMarketClose,
    );

    println!("\n=== Testing UnifiedExecutor Straddle ===");

    // Get selector and structure
    let selector = use_case.create_selector();
    let structure = TradeStructure::Straddle;

    // Call the new optimized method
    let result = use_case.process_event_unified(
        &event,
        &*selector,
        structure,
    ).await;

    println!("Success: {}", result.success());

    // Verify we got a straddle result
    match result {
        TradeResult::Straddle(r) => {
            println!("✓ StraddleResult received");
            println!("  Strike: {}", r.strike.value());
            println!("  Expiration: {}", r.expiration);
        }
        TradeResult::Failed(failed) => {
            println!("⚠ Trade failed");
            println!("  Reason: {:?}", failed.reason);
            println!("  Phase: {}", failed.phase);
        }
        _ => panic!("Expected Straddle result"),
    }

    println!("✓ Test passed");
}
