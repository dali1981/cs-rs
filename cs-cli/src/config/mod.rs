//! Configuration building from CLI args

mod app;
mod builder;

// Re-export everything from app module
pub use app::*;

// Re-export builder
pub use builder::BacktestConfigBuilder;
