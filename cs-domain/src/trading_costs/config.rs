//! Trading costs configuration
//!
//! Serde-compatible configuration for trading cost models.
//! Supports TOML, JSON, and other serde formats.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::TradingCostCalculator;
use super::models::{
    NoCost, FixedPerLegSlippage, PercentageOfPremiumSlippage,
    HalfSpreadSlippage, IVBasedSlippage, CommissionModel, CompositeCostCalculator,
};

/// Configuration for trading costs
///
/// Deserializable from TOML/JSON for easy configuration.
///
/// # TOML Examples
///
/// ```toml
/// # Simple preset
/// [trading_costs]
/// model = "preset"
/// name = "normal"
///
/// # Fixed per leg
/// [trading_costs]
/// model = "fixed_per_leg"
/// cost_per_leg = "0.02"
///
/// # Half-spread
/// [trading_costs]
/// model = "half_spread"
/// spread_pct = 0.04
///
/// # Composite (slippage + commission)
/// [trading_costs]
/// model = "composite"
///
/// [trading_costs.slippage]
/// model = "half_spread"
/// spread_pct = 0.04
///
/// [trading_costs.commission]
/// model = "commission"
/// per_contract = "0.65"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum TradingCostConfig {
    /// No costs
    None,

    /// Fixed cost per leg
    FixedPerLeg {
        /// Cost per leg per share (e.g., "0.02" for $0.02)
        cost_per_leg: Decimal,
    },

    /// Percentage of premium
    Percentage {
        /// Slippage in basis points (e.g., 50 for 0.50%)
        slippage_bps: u32,
        /// Minimum cost per leg (optional)
        #[serde(default)]
        min_cost_per_leg: Option<Decimal>,
        /// Maximum cost per leg (optional)
        #[serde(default)]
        max_cost_per_leg: Option<Decimal>,
    },

    /// IV-based spread
    IvBased {
        /// Base spread percentage (e.g., 0.02 for 2%)
        base_spread_pct: f64,
        /// How much spread widens per unit of IV
        iv_multiplier: f64,
        /// Maximum spread percentage (default 20%)
        #[serde(default = "default_max_spread")]
        max_spread_pct: f64,
    },

    /// Half-spread model
    HalfSpread {
        /// Assumed bid-ask spread as percentage (e.g., 0.04 for 4%)
        spread_pct: f64,
    },

    /// Commission only
    Commission {
        /// Commission per contract (e.g., "0.65" for $0.65)
        per_contract: Decimal,
        /// Maximum commission per leg (optional)
        #[serde(default)]
        max_per_leg: Option<Decimal>,
    },

    /// Composite (slippage + commission)
    Composite {
        /// Slippage configuration
        slippage: Box<TradingCostConfig>,
        /// Commission configuration
        commission: Box<TradingCostConfig>,
    },

    /// Preset configurations
    Preset {
        /// Which preset to use
        name: CostPreset,
    },
}

fn default_max_spread() -> f64 { 0.20 }

/// Preset cost configurations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostPreset {
    /// Tight markets, low costs
    Tight,
    /// Normal market conditions
    Normal,
    /// Wide spreads, illiquid
    Wide,
    /// Realistic (slippage + commission)
    Realistic,
    /// IBKR-like costs
    Ibkr,
    /// Tastytrade-like costs
    Tastytrade,
    /// IV-sensitive (for earnings trades)
    IvSensitive,
}

impl TradingCostConfig {
    /// Build the calculator from config
    pub fn build(&self) -> Box<dyn TradingCostCalculator> {
        match self {
            Self::None => Box::new(NoCost),

            Self::FixedPerLeg { cost_per_leg } =>
                Box::new(FixedPerLegSlippage::new(*cost_per_leg)),

            Self::Percentage { slippage_bps, min_cost_per_leg, max_cost_per_leg } => {
                let mut model = PercentageOfPremiumSlippage::new(*slippage_bps);
                if let Some(min) = min_cost_per_leg {
                    model.min_cost_per_leg = *min;
                }
                model.max_cost_per_leg = *max_cost_per_leg;
                Box::new(model)
            }

            Self::IvBased { base_spread_pct, iv_multiplier, max_spread_pct } => {
                let mut model = IVBasedSlippage::new(*base_spread_pct, *iv_multiplier);
                model.max_spread_pct = *max_spread_pct;
                Box::new(model)
            }

            Self::HalfSpread { spread_pct } =>
                Box::new(HalfSpreadSlippage::new(*spread_pct)),

            Self::Commission { per_contract, max_per_leg } => {
                let mut model = CommissionModel::new(*per_contract);
                model.max_per_leg = *max_per_leg;
                Box::new(model)
            }

            Self::Composite { slippage, commission } => {
                Box::new(CompositeCostCalculator::new()
                    .with_boxed(slippage.build())
                    .with_boxed(commission.build()))
            }

            Self::Preset { name } => name.build(),
        }
    }

    /// Create a "no cost" config
    pub fn none() -> Self {
        Self::None
    }

    /// Create a preset config
    pub fn preset(name: CostPreset) -> Self {
        Self::Preset { name }
    }

    /// Create a half-spread config
    pub fn half_spread(spread_pct: f64) -> Self {
        Self::HalfSpread { spread_pct }
    }

    /// Create a fixed-per-leg config
    pub fn fixed_per_leg(cost_per_leg: Decimal) -> Self {
        Self::FixedPerLeg { cost_per_leg }
    }

    /// Check if this config will produce non-zero costs
    pub fn has_costs(&self) -> bool {
        !matches!(self, Self::None)
    }
}

impl CostPreset {
    /// Build the calculator for this preset
    pub fn build(&self) -> Box<dyn TradingCostCalculator> {
        match self {
            Self::Tight => Box::new(HalfSpreadSlippage::tight()),
            Self::Normal => Box::new(HalfSpreadSlippage::normal()),
            Self::Wide => Box::new(HalfSpreadSlippage::wide()),
            Self::Realistic => Box::new(CompositeCostCalculator::realistic()),
            Self::Ibkr => Box::new(CompositeCostCalculator::ibkr()),
            Self::Tastytrade => Box::new(CompositeCostCalculator::tastytrade()),
            Self::IvSensitive => Box::new(IVBasedSlippage::moderate()),
        }
    }
}

impl Default for TradingCostConfig {
    fn default() -> Self {
        Self::None  // Explicit opt-in for costs
    }
}

impl Default for CostPreset {
    fn default() -> Self {
        Self::Normal
    }
}

impl std::fmt::Display for CostPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tight => write!(f, "tight"),
            Self::Normal => write!(f, "normal"),
            Self::Wide => write!(f, "wide"),
            Self::Realistic => write!(f, "realistic"),
            Self::Ibkr => write!(f, "ibkr"),
            Self::Tastytrade => write!(f, "tastytrade"),
            Self::IvSensitive => write!(f, "iv_sensitive"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_config_none() {
        let config = TradingCostConfig::None;
        let calc = config.build();
        assert_eq!(calc.name(), "NoCost");
    }

    #[test]
    fn test_config_preset() {
        let config = TradingCostConfig::Preset { name: CostPreset::Normal };
        let calc = config.build();
        assert_eq!(calc.name(), "HalfSpread");
    }

    #[test]
    fn test_config_composite() {
        let config = TradingCostConfig::Composite {
            slippage: Box::new(TradingCostConfig::HalfSpread { spread_pct: 0.04 }),
            commission: Box::new(TradingCostConfig::Commission {
                per_contract: dec!(0.65),
                max_per_leg: Some(dec!(10.00)),
            }),
        };
        let calc = config.build();
        assert_eq!(calc.name(), "Composite");
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = TradingCostConfig::HalfSpread { spread_pct: 0.04 };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: TradingCostConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
            model = "half_spread"
            spread_pct = 0.04
        "#;
        let config: TradingCostConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config, TradingCostConfig::HalfSpread { spread_pct: 0.04 });
    }

    #[test]
    fn test_config_preset_from_toml() {
        let toml_str = r#"
            model = "preset"
            name = "realistic"
        "#;
        let config: TradingCostConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config, TradingCostConfig::Preset { name: CostPreset::Realistic });
    }

    #[test]
    fn test_config_composite_from_toml() {
        let toml_str = r#"
            model = "composite"

            [slippage]
            model = "half_spread"
            spread_pct = 0.04

            [commission]
            model = "commission"
            per_contract = "0.65"
        "#;
        let config: TradingCostConfig = toml::from_str(toml_str).unwrap();

        match config {
            TradingCostConfig::Composite { slippage, commission } => {
                assert!(matches!(*slippage, TradingCostConfig::HalfSpread { .. }));
                assert!(matches!(*commission, TradingCostConfig::Commission { .. }));
            }
            _ => panic!("Expected Composite config"),
        }
    }
}
