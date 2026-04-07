// Translation layer — all external → domain mappings live here (ADR-0001)
pub mod mappers;
pub use mappers::IntoNormalized;

// Finq-based repositories (requires finq-flatfiles)
#[cfg(feature = "finq-flatfiles")]
pub mod finq_options_repo;
#[cfg(feature = "finq-flatfiles")]
pub mod finq_equity_repo;
pub mod ib_options_repo;
pub mod ib_equity_repo;

// Earnings repositories
pub mod earnings_repo;
#[cfg(feature = "earnings-rs")]
pub mod earnings_reader_adapter;
pub mod custom_file_earnings;
pub mod parquet_results_repo;

// Demo repositories (always available, used when demo feature is on)
pub mod demo_repos;

// Re-exports
#[cfg(feature = "finq-flatfiles")]
pub use finq_options_repo::FinqOptionsRepository;
#[cfg(feature = "finq-flatfiles")]
pub use finq_equity_repo::FinqEquityRepository;
pub use ib_options_repo::IbOptionsRepository;
pub use ib_equity_repo::IbEquityRepository;
pub use earnings_repo::{StubEarningsRepository, ParquetEarningsRepository};
#[cfg(feature = "earnings-rs")]
pub use earnings_reader_adapter::EarningsReaderAdapter;
pub use custom_file_earnings::CustomFileEarningsReader;
pub use parquet_results_repo::ParquetResultsRepository;

// Demo re-exports
pub use demo_repos::{DemoOptionsRepository, DemoEquityRepository, DemoEarningsRepository};
