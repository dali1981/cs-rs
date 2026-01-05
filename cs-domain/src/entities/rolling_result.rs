use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Result of a rolling strategy (any trade type)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingResult {
    pub symbol: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub roll_policy: String,
    pub trade_type: String,  // "straddle", "calendar_spread", etc.

    // Individual roll periods
    pub rolls: Vec<RollPeriod>,

    // Aggregated metrics
    pub total_option_pnl: Decimal,
    pub total_hedge_pnl: Decimal,
    pub total_transaction_cost: Decimal,
    pub total_pnl: Decimal,

    // Statistics
    pub num_rolls: usize,
    pub win_rate: f64,
    pub avg_roll_pnl: Decimal,
    pub max_drawdown: Decimal,
}

impl RollingResult {
    /// Create a new result from a list of roll periods
    pub fn from_rolls(
        symbol: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        roll_policy: String,
        trade_type: String,
        rolls: Vec<RollPeriod>,
    ) -> Self {
        let num_rolls = rolls.len();

        let total_option_pnl: Decimal = rolls.iter().map(|r| r.pnl).sum();
        let total_hedge_pnl: Decimal = rolls.iter()
            .filter_map(|r| r.hedge_pnl)
            .sum();
        let total_transaction_cost: Decimal = rolls.iter()
            .map(|r| r.transaction_cost)
            .sum();
        let total_pnl = total_option_pnl + total_hedge_pnl - total_transaction_cost;

        let winners = rolls.iter().filter(|r| r.pnl > Decimal::ZERO).count();
        let win_rate = if num_rolls > 0 {
            winners as f64 / num_rolls as f64
        } else {
            0.0
        };

        let avg_roll_pnl = if num_rolls > 0 {
            total_pnl / Decimal::from(num_rolls)
        } else {
            Decimal::ZERO
        };

        // Calculate max drawdown
        let mut peak = Decimal::ZERO;
        let mut max_drawdown = Decimal::ZERO;
        let mut cumulative_pnl = Decimal::ZERO;

        for roll in &rolls {
            cumulative_pnl += roll.pnl;
            if let Some(hedge_pnl) = roll.hedge_pnl {
                cumulative_pnl += hedge_pnl;
            }
            cumulative_pnl -= roll.transaction_cost;

            if cumulative_pnl > peak {
                peak = cumulative_pnl;
            }
            let drawdown = peak - cumulative_pnl;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }

        Self {
            symbol,
            start_date,
            end_date,
            roll_policy,
            trade_type,
            rolls,
            total_option_pnl,
            total_hedge_pnl,
            total_transaction_cost,
            total_pnl,
            num_rolls,
            win_rate,
            avg_roll_pnl,
            max_drawdown,
        }
    }
}

/// A single roll period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollPeriod {
    pub entry_date: NaiveDate,
    pub exit_date: NaiveDate,
    pub strike: Decimal,
    pub expiration: NaiveDate,

    pub entry_debit: Decimal,
    pub exit_credit: Decimal,
    pub pnl: Decimal,

    pub spot_at_entry: f64,
    pub spot_at_exit: f64,
    pub spot_move_pct: f64,

    pub iv_entry: Option<f64>,
    pub iv_exit: Option<f64>,
    pub iv_change: Option<f64>,

    pub net_delta: Option<f64>,
    pub net_gamma: Option<f64>,
    pub net_theta: Option<f64>,
    pub net_vega: Option<f64>,

    pub hedge_pnl: Option<Decimal>,
    pub hedge_count: usize,
    pub transaction_cost: Decimal,

    pub roll_reason: RollReason,
}

/// Reason why a position was rolled
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RollReason {
    /// Normal scheduled roll (weekly/monthly/days)
    Scheduled,
    /// Option expired before roll date
    Expiry,
    /// Reached campaign end date
    EndOfCampaign,
}

impl std::fmt::Display for RollReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scheduled => write!(f, "Scheduled"),
            Self::Expiry => write!(f, "Expiry"),
            Self::EndOfCampaign => write!(f, "End"),
        }
    }
}
