//! Position attribution module for P&L decomposition
//!
//! Collects daily snapshots of position Greeks and computes P&L attribution
//! by delta, gamma, theta, and vega contributions.

mod greeks_computer;
mod snapshot_collector;

pub use greeks_computer::GreeksComputer;
pub use snapshot_collector::SnapshotCollector;

use std::sync::Arc;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use cs_domain::{
    AttributionConfig, CompositeTrade, EquityDataRepository, HedgePosition,
    OptionsDataRepository, PositionAttribution,
};

/// Compute P&L attribution from hedge history (standalone function)
///
/// This extracts the attribution computation logic from TradeExecutor,
/// enabling reuse across different execution paths (backtest, rolling, etc.).
///
/// # Arguments
/// * `trade` - The composite trade (provides legs, symbol, expiration)
/// * `hedge_position` - Completed hedge position with hedge transactions
/// * `entry_time` - Trade entry time
/// * `exit_time` - Trade exit time
/// * `actual_pnl` - Actual P&L for unexplained calculation
/// * `attribution_config` - Configuration for snapshot collection
/// * `options_repo` - Options data repository for IV surfaces
/// * `equity_repo` - Equity data repository for spot prices
/// * `contract_multiplier` - Contract multiplier (default: 100)
///
/// # Returns
/// `PositionAttribution` with delta/gamma/theta/vega P&L decomposition
pub async fn compute_position_attribution<T: CompositeTrade + Clone>(
    trade: T,
    hedge_position: &HedgePosition,
    entry_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    actual_pnl: Decimal,
    attribution_config: &AttributionConfig,
    options_repo: Arc<dyn OptionsDataRepository>,
    equity_repo: Arc<dyn EquityDataRepository>,
    contract_multiplier: i32,
) -> Result<PositionAttribution, String> {
    let symbol = CompositeTrade::symbol(&trade).to_string();

    // Create snapshot collector
    let mut collector = SnapshotCollector::new(
        trade,
        options_repo,
        equity_repo,
        symbol,
        attribution_config.clone(),
        contract_multiplier,
        0.05, // risk_free_rate
    );

    // Set hedge timeline from completed hedging
    collector.set_hedge_timeline(&hedge_position.hedges);

    // TODO: Set entry vol for EntryIV/EntryHV modes
    // This would require passing entry IV/HV from the hedging phase

    // Collect daily snapshots
    collector.collect(entry_time, exit_time).await?;

    // Build attribution
    collector
        .build_attribution(actual_pnl)
        .ok_or_else(|| "No snapshots collected for attribution".to_string())
}
