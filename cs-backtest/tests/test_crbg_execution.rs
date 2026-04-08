/// Integration test for CRBG calendar spread execution
///
/// This test reproduces the exact conditions from the failed backtest:
/// - Symbol: CRBG
/// - Entry: 2025-11-03 20:55 UTC (15:55 ET)
/// - Exit: 2025-11-04 14:45 UTC (9:45 ET)
/// - Spread: Calendar Call
/// - Strike: ATM (~31)
/// - Short expiry: 2025-11-21 (18 DTE)
/// - Long expiry: 2025-12-19 (46 DTE)
///
/// Requires finq-flatfiles feature and real data at ~/polygon/data.

#[cfg(feature = "finq-flatfiles")]
mod crbg_tests {
    use chrono::{DateTime, NaiveDate, Utc};
    use cs_backtest::TradeExecutor;
    use cs_domain::{
        infrastructure::{FinqEquityRepository, FinqOptionsRepository},
        CalendarSpread, Strike, OptionLeg,
        EquityDataRepository, OptionsDataRepository,
        testing::EarningsEventBuilder,
    };
    use finq_core::OptionType;
    use std::path::PathBuf;
    use std::sync::Arc;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn test_crbg_calendar_spread_execution() {
        // Setup
        let data_dir = PathBuf::from("/Users/mohamedali/polygon/data");
        let options_repo = Arc::new(FinqOptionsRepository::new(data_dir.clone()));
        let equity_repo = Arc::new(FinqEquityRepository::new(data_dir));

        let executor = TradeExecutor::new(
            options_repo.clone(),
            equity_repo.clone(),
        );

        // Trade parameters from failed backtest
        let symbol = "CRBG";
        let entry_time = DateTime::parse_from_rfc3339("2025-11-03T20:55:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let exit_time = DateTime::parse_from_rfc3339("2025-11-04T14:45:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // Calendar spread: ATM call
        let short_expiry = NaiveDate::from_ymd_opt(2025, 11, 21).unwrap();
        let long_expiry = NaiveDate::from_ymd_opt(2025, 12, 19).unwrap();
        let strike = Strike::new(Decimal::new(31, 0)).unwrap(); // ATM strike

        println!("=== CRBG Calendar Spread Execution Test ===");
        println!("Symbol: {}", symbol);
        println!("Entry: {}", entry_time);
        println!("Exit: {}", exit_time);
        println!("Strike: {}", strike.value());
        println!("Short expiry: {}", short_expiry);
        println!("Long expiry: {}", long_expiry);
        println!();

        // Step 1: Check spot prices
        println!("Step 1: Checking spot prices...");
        match equity_repo.get_spot_price(symbol, entry_time).await {
            Ok(spot) => println!("  ✓ Entry spot: ${}", spot.to_f64()),
            Err(e) => {
                println!("  ✗ Entry spot FAILED: {}", e);
                panic!("Cannot get entry spot price");
            }
        }

        match equity_repo.get_spot_price(symbol, exit_time).await {
            Ok(spot) => println!("  ✓ Exit spot: ${}", spot.to_f64()),
            Err(e) => println!("  ! Exit spot failed: {} (expected for illiquid exit)", e),
        }
        println!();

        // Step 2: Check option chain at entry
        println!("Step 2: Checking option chain at entry...");
        match options_repo.get_option_bars_at_time(symbol, entry_time).await {
            Ok(chain) => {
                println!("  ✓ Entry chain loaded: {} bars", chain.len());

                let call_bars: Vec<_> = chain.iter()
                    .filter(|b| matches!(b.option_type, OptionType::Call))
                    .collect();
                println!("  Call bars: {}", call_bars.len());

                // Show available strikes
                if !call_bars.is_empty() {
                    let mut unique_strikes: Vec<f64> = call_bars.iter()
                        .map(|b| b.strike)
                        .collect();
                    unique_strikes.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    unique_strikes.dedup();
                    println!("  Available call strikes: {:?}", unique_strikes);

                    let strike_31_calls: Vec<_> = call_bars.iter()
                        .filter(|b| (b.strike - 31.0).abs() < 0.001)
                        .collect();

                    if !strike_31_calls.is_empty() {
                        println!("  ✓ Strike 31 call bars found: {}", strike_31_calls.len());
                        for bar in strike_31_calls.iter().take(5) {
                            println!("    exp={} price={:?}", bar.expiration, bar.close);
                        }
                    } else {
                        println!("  ✗ NO strike 31 call bars found!");
                    }
                }
            }
            Err(e) => {
                println!("  ✗ Entry chain FAILED: {}", e);
                panic!("Cannot load entry option chain");
            }
        }
        println!();

        // Step 3: Build IV surface with detailed diagnostics
        println!("Step 3: Building IV surface at entry...");
        match options_repo.get_option_bars_at_time(symbol, entry_time).await {
            Ok(chain) => {
                println!("  Chain has {} total bars", chain.len());

                let mut successful_iv_count = 0;

                for (i, bar) in chain.iter().enumerate().take(13) {
                    if let (Some(close), Some(ts)) = (bar.close, bar.timestamp) {
                        match equity_repo.get_spot_price(symbol, ts).await {
                            Ok(spot) => {
                                println!("    Bar {}: strike={} {:?} close=${:.2} ts={} spot=${:.2} ✓",
                                    i, bar.strike, bar.option_type, close,
                                    ts.format("%H:%M"), spot.to_f64());
                                successful_iv_count += 1;
                            }
                            Err(e) => {
                                println!("    Bar {}: strike={} {:?} close=${:.2} ts={} SPOT LOOKUP FAILED: {}",
                                    i, bar.strike, bar.option_type, close,
                                    ts.format("%H:%M"), e);
                            }
                        }
                    }
                }

                println!("  Bars with successful spot lookup: {}/{}", successful_iv_count, chain.len());
                println!();

                let surface = cs_backtest::build_iv_surface_minute_aligned(
                    &chain,
                    equity_repo.as_ref(),
                    symbol,
                ).await;

                match surface {
                    Some(surf) => {
                        println!("  ✓ IV surface built: {} points", surf.points().len());
                        println!("  All IV points:");
                        for p in surf.points() {
                            let is_call_str = if p.is_call { "call" } else { "put" };
                            println!("    Strike {} {} exp={} IV={:.1}%",
                                p.strike, is_call_str, p.expiration, p.iv * 100.0);
                        }
                    }
                    None => {
                        println!("  ✗ IV surface build returned None - all bars were filtered out!");
                    }
                }
            }
            Err(e) => println!("  ✗ Cannot load chain: {}", e),
        }
        println!();

        // Step 4: Execute trade
        println!("Step 4: Executing calendar spread trade...");

        let short_leg = OptionLeg {
            symbol: symbol.to_string(),
            strike,
            option_type: OptionType::Call,
            expiration: short_expiry,
        };

        let long_leg = OptionLeg {
            symbol: symbol.to_string(),
            strike,
            option_type: OptionType::Call,
            expiration: long_expiry,
        };

        let spread = CalendarSpread::new(short_leg, long_leg).unwrap();

        let earnings_event = EarningsEventBuilder::new(symbol)
            .earnings_date(NaiveDate::from_ymd_opt(2025, 11, 3).unwrap())
            .company_name("CRBG")
            .build();

        let result = executor.execute_trade(
            &spread,
            &earnings_event,
            entry_time,
            exit_time,
        ).await;

        println!();
        println!("=== TRADE RESULT ===");
        println!("Success: {}", result.success);

        if !result.success {
            println!("Failure reason: {:?}", result.failure_reason);
        } else {
            println!("Entry cost: ${}", result.entry_cost);
            println!("Exit value: ${}", result.exit_value);
            println!("P&L: ${} ({:.2}%)", result.pnl, result.pnl_pct);
        }
        println!();

        // The test assertion
        if !result.success {
            if let Some(reason) = result.failure_reason {
                panic!("Trade failed: {:?}", reason);
            }
        }
    }
}
