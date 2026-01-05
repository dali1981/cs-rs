use crate::expiration::ExpirationPolicy;
use crate::trading_period::TradingPeriodSpec;
use crate::roll::RollPolicy;
use crate::hedging::HedgeConfig;
use crate::strike_selection::StrikeMatchMode;
use finq_core::OptionType;

/// Complete trade strategy configuration
#[derive(Debug, Clone)]
pub struct TradeStrategy {
    /// Trade structure (straddle, calendar, etc.)
    pub structure: TradeStructureConfig,

    /// Timing specification (when to enter/exit)
    pub timing: TradingPeriodSpec,

    /// Expiration selection policy
    pub expiration_policy: ExpirationPolicy,

    /// Roll policy for multi-period trades
    pub roll_policy: RollPolicy,

    /// Delta hedging configuration
    pub hedge_config: HedgeConfig,

    /// Entry/exit filters
    pub filters: TradeFilters,
}

/// Trade structure configuration
#[derive(Debug, Clone)]
pub enum TradeStructureConfig {
    /// Long straddle (ATM call + ATM put)
    Straddle,

    /// Calendar spread
    CalendarSpread {
        option_type: OptionType,
        strike_match: StrikeMatchMode,
    },

    /// Calendar straddle (4 legs)
    CalendarStraddle,

    /// Iron butterfly
    IronButterfly {
        wing_width: rust_decimal::Decimal,
    },
}

/// Filters for trade entry
#[derive(Debug, Clone, Default)]
pub struct TradeFilters {
    /// Minimum IV to enter
    pub min_iv: Option<f64>,

    /// Maximum IV to enter
    pub max_iv: Option<f64>,

    /// Minimum IV ratio (short/long) for calendars
    pub min_iv_ratio: Option<f64>,

    /// Minimum option volume
    pub min_volume: Option<u64>,

    /// Maximum bid-ask spread percentage
    pub max_bid_ask_pct: Option<f64>,
}

impl Default for TradeStrategy {
    fn default() -> Self {
        Self {
            structure: TradeStructureConfig::Straddle,
            timing: TradingPeriodSpec::pre_earnings_default(),
            expiration_policy: ExpirationPolicy::FirstAfter {
                min_date: chrono::NaiveDate::MIN,
            },
            roll_policy: RollPolicy::None,
            hedge_config: HedgeConfig::default(),
            filters: TradeFilters::default(),
        }
    }
}

impl TradeStrategy {
    /// Create a new strategy with the given structure
    pub fn new(structure: TradeStructureConfig) -> Self {
        Self {
            structure,
            ..Default::default()
        }
    }

    /// Set timing specification
    pub fn with_timing(mut self, timing: TradingPeriodSpec) -> Self {
        self.timing = timing;
        self
    }

    /// Set expiration policy
    pub fn with_expiration_policy(mut self, policy: ExpirationPolicy) -> Self {
        self.expiration_policy = policy;
        self
    }

    /// Set roll policy
    pub fn with_roll_policy(mut self, policy: RollPolicy) -> Self {
        self.roll_policy = policy;
        self
    }

    /// Set hedge config
    pub fn with_hedge_config(mut self, config: HedgeConfig) -> Self {
        self.hedge_config = config;
        self
    }

    /// Set filters
    pub fn with_filters(mut self, filters: TradeFilters) -> Self {
        self.filters = filters;
        self
    }
}
