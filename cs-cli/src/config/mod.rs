//! Configuration building from CLI args

mod app;
mod builder;
#[cfg(feature = "experimental-cli")]
mod campaign_builder;

// Re-export everything from app module
pub use app::*;

// Re-export builders
pub use builder::BacktestConfigBuilder;
#[cfg(feature = "experimental-cli")]
pub use campaign_builder::CampaignConfigBuilder;
