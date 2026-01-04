// cs-analytics: Pure computational functions for options analytics
//
// No I/O, no side effects - highly testable and parallelizable.

pub mod black_scholes;
pub mod greeks;
pub mod iv_model;
pub mod iv_surface;
pub mod realized_volatility;
pub mod iv_statistics;
pub mod math_utils;
pub mod vol_slice;
pub mod delta_surface;
pub mod opportunity;
pub mod selection_model;
pub mod svi;
pub mod svi_fitter;
pub mod arbitrage;
pub mod atm_iv_computer;
pub mod straddle;
pub mod pnl_attribution;

pub use black_scholes::{bs_price, bs_implied_volatility, bs_greeks, bs_delta, BSConfig, BSError};
pub use greeks::Greeks;
pub use iv_model::{
    PricingIVProvider, PricingModel,
    StickyStrikePricing, StickyMoneynessPricing, StickyDeltaPricing,
};
pub use iv_surface::{IVSurface, IVPoint};
pub use realized_volatility::realized_volatility;
pub use iv_statistics::{iv_percentile, iv_rank};
pub use math_utils::{inv_norm_cdf, linspace};
pub use vol_slice::{VolSlice, InterpolationMode, delta_to_strike_with_iv};
pub use delta_surface::DeltaVolSurface;
pub use opportunity::{CalendarOpportunity, OpportunityAnalyzer, OpportunityAnalyzerConfig};
pub use selection_model::{
    SelectionModel, SelectionIVProvider, SelectionIVPair,
    StrikeSpaceSelection, DeltaSpaceSelection,
};
pub use svi::{SVIParams, SVIError};
pub use svi_fitter::{SVIFitter, SVIFitterConfig};
pub use arbitrage::{
    ArbitrageViolation, ArbitrageReport,
    check_butterfly_arbitrage, check_calendar_arbitrage, full_arbitrage_check,
};
pub use atm_iv_computer::{
    AtmIvComputer, AtmIvResult, AtmMethod, OptionPoint,
    ExpirationIv, ConstantMaturityResult, ConstantMaturityInterpolator,
};
pub use straddle::{StraddlePriceComputer, StraddlePrice};
pub use pnl_attribution::{
    PnLAttribution, LegPnL,
    calculate_pnl_attribution, calculate_spread_pnl_attribution, calculate_option_leg_pnl,
};
