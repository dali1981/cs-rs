// cs-analytics: Pure computational functions for options analytics
//
// No I/O, no side effects - highly testable and parallelizable.

pub mod arbitrage;
pub mod atm_iv_computer;
pub mod black_scholes;
pub mod delta_surface;
pub mod greeks;
pub mod iv_model;
pub mod iv_statistics;
pub mod iv_surface;
pub mod math_utils;
pub mod opportunity;
pub mod pnl_attribution;
pub mod realized_volatility;
pub mod selection_model;
pub mod straddle;
pub mod svi;
pub mod svi_fitter;
pub mod vol_slice;

pub use arbitrage::{
    check_butterfly_arbitrage, check_calendar_arbitrage, full_arbitrage_check, ArbitrageReport,
    ArbitrageViolation,
};
pub use atm_iv_computer::{
    AtmIvComputer, AtmIvResult, AtmMethod, ConstantMaturityInterpolator, ConstantMaturityResult,
    ExpirationIv, OptionPoint,
};
pub use black_scholes::{bs_delta, bs_greeks, bs_implied_volatility, bs_price, BSConfig, BSError};
pub use delta_surface::DeltaVolSurface;
pub use greeks::Greeks;
pub use iv_model::{
    PricingIVProvider, PricingModel, StickyDeltaPricing, StickyMoneynessPricing,
    StickyStrikePricing,
};
pub use iv_statistics::{iv_percentile, iv_rank};
pub use iv_surface::{IVPoint, IVSurface};
pub use math_utils::{inv_norm_cdf, linspace};
pub use opportunity::{CalendarOpportunity, OpportunityAnalyzer, OpportunityAnalyzerConfig};
pub use pnl_attribution::{
    calculate_option_leg_pnl, calculate_pnl_attribution, calculate_spread_pnl_attribution, LegPnL,
    PnLAttribution,
};
pub use realized_volatility::realized_volatility;
pub use selection_model::{
    DeltaSpaceSelection, SelectionIVPair, SelectionIVProvider, SelectionModel, StrikeSpaceSelection,
};
pub use straddle::{StraddlePrice, StraddlePriceComputer};
pub use svi::{SVIError, SVIParams};
pub use svi_fitter::{SVIFitter, SVIFitterConfig};
pub use vol_slice::{delta_to_strike_with_iv, InterpolationMode, VolSlice};
