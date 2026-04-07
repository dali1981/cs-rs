//! Application command types for the backtest context.
//!
//! `RunBacktestCommand` is the typed, validated representation of a user's intent
//! to run a backtest. It is composed of bounded sub-structures, each owning one
//! concern: period, strategy, execution, filters, risk.
//!
//! This type is the stable handoff between the Application layer (cs-cli) and the
//! Execution layer (BacktestUseCase). When the use case is eventually migrated to
//! accept this type directly, the temporary shim in `UseCaseFactory` can be removed.
//!
//! Does NOT contain:
//! - `DataSourceConfig` — infrastructure, stays in the factory
//! - `EarningsSourceConfig` — infrastructure, stays in the factory
//! - `data_dir` — deprecated field, never populated from this type
//! - `serde` derives — this type is not a config file format
//!
//! See ADR-0001 (bounded contexts) and ADR-0003 (CLI/config are DTOs).

use chrono::NaiveDate;
use cs_analytics::{PricingModel, InterpolationMode};
use cs_domain::{
    TimingConfig, TradeSelectionCriteria, StrikeMatchMode, HedgeConfig, AttributionConfig,
    TradingCostConfig, FileRulesConfig, ReturnBasis, MarginConfig,
};

use crate::config::{SpreadType, SelectionType};

// ── Sub-structures ────────────────────────────────────────────────────────────

/// The date range over which the backtest runs.
#[derive(Debug, Clone)]
pub struct BacktestPeriod {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

/// What to trade and when — spread type, selection, and timing.
#[derive(Debug, Clone)]
pub struct StrategySpec {
    // Spread and selection
    pub spread: SpreadType,
    pub selection_strategy: SelectionType,
    pub selection: TradeSelectionCriteria,

    // Entry/exit timing
    pub timing: TimingConfig,
    /// Named timing strategy (e.g. "PreEarnings"). Overrides legacy timing params when set.
    pub timing_strategy: Option<String>,
    pub entry_days_before: Option<u16>,
    pub exit_days_before: Option<u16>,
    pub entry_offset: Option<i16>,
    pub holding_days: Option<u16>,
    pub exit_days_after: Option<u16>,

    // Strategy-specific params
    pub wing_width: f64,
    pub straddle_entry_days: usize,
    pub straddle_exit_days: usize,
    pub min_straddle_dte: i32,
    pub post_earnings_holding_days: usize,
}

/// How trades are priced and executed — model selection, delta targeting, parallelism.
///
/// Named `ExecutionSpec` (not `ExecutionConfig`) to avoid ambiguity with
/// `crate::execution::ExecutionConfig`, which governs the trade execution engine.
#[derive(Debug, Clone)]
pub struct ExecutionSpec {
    pub parallel: bool,
    pub pricing_model: PricingModel,
    pub vol_model: InterpolationMode,
    pub target_delta: f64,
    pub delta_range: (f64, f64),
    pub delta_scan_steps: usize,
    pub strike_match_mode: StrikeMatchMode,
}

/// Which events and trades pass entry — symbol lists, market-cap gates, price/IV bounds.
#[derive(Debug, Clone)]
pub struct FilterSet {
    pub symbols: Option<Vec<String>>,
    pub min_market_cap: Option<u64>,
    pub max_entry_iv: Option<f64>,
    pub min_notional: Option<f64>,
    pub min_entry_price: Option<f64>,
    pub max_entry_price: Option<f64>,
    pub rules: FileRulesConfig,
}

/// Return measurement, margin, hedging, attribution, and cost assumptions.
#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub return_basis: ReturnBasis,
    pub margin: MarginConfig,
    pub hedge_config: HedgeConfig,
    pub attribution_config: Option<AttributionConfig>,
    pub trading_costs: TradingCostConfig,
}

// ── Top-level command ─────────────────────────────────────────────────────────

/// A validated, typed application command to run a backtest.
///
/// Composed of five bounded sub-structures, each owning one concern.
/// Produced by the CLI/config layer after parsing and normalization.
/// Consumed by [`crate::factory::UseCaseFactory`] to create a [`crate::BacktestUseCase`].
#[derive(Debug, Clone)]
pub struct RunBacktestCommand {
    pub period: BacktestPeriod,
    pub strategy: StrategySpec,
    pub execution: ExecutionSpec,
    pub filters: FilterSet,
    pub risk: RiskConfig,
}
