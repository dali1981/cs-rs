//! Factory for creating use cases with proper dependencies

#[cfg(feature = "full")]
mod ib_repository_factory;
mod repository_factory;
mod use_case_factory;

#[cfg(feature = "full")]
pub use ib_repository_factory::IbRepositoryFactory;
pub use repository_factory::{DataRepositoryFactory, RepositoryFactory};
pub use use_case_factory::UseCaseFactory;
