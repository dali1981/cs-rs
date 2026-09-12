// Translation layer — all external → domain mappings live here (ADR-0001)
pub mod mappers;
// Private DataFrame → domain type converters (not part of public API)
mod option_bar_conversions;
pub use mappers::IntoNormalized;

// Finq-based repositories (requires finq-flatfiles)
#[cfg(feature = "finq-flatfiles")]
pub mod finq_equity_repo;
#[cfg(feature = "finq-flatfiles")]
pub mod finq_options_repo;
pub mod ib_equity_repo;
pub mod ib_options_repo;

// Earnings repositories
pub mod custom_file_earnings;
#[cfg(feature = "earnings-rs")]
pub mod earnings_reader_adapter;
pub mod earnings_repo;
pub mod parquet_results_repo;

// Demo repositories (always available, used when demo feature is on)
pub mod demo_repos;

// Re-exports
pub use custom_file_earnings::CustomFileEarningsReader;
#[cfg(feature = "earnings-rs")]
pub use earnings_reader_adapter::EarningsReaderAdapter;
pub use earnings_repo::{ParquetEarningsRepository, StubEarningsRepository};
#[cfg(feature = "finq-flatfiles")]
pub use finq_equity_repo::FinqEquityRepository;
#[cfg(feature = "finq-flatfiles")]
pub use finq_options_repo::FinqOptionsRepository;
pub use ib_equity_repo::IbEquityRepository;
pub use ib_options_repo::IbOptionsRepository;
pub use parquet_results_repo::ParquetResultsRepository;

// Demo re-exports
pub use demo_repos::{DemoEarningsRepository, DemoEquityRepository, DemoOptionsRepository};
