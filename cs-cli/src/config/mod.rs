//! Configuration building from CLI args

mod app;
mod builder;
mod campaign_builder;

// Re-export everything from app module
pub use app::*;

// Re-export builders
pub use builder::BacktestConfigBuilder;
pub use campaign_builder::CampaignConfigBuilder;
