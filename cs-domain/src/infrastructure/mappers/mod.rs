//! Translation layer — maps external provider data into canonical domain models.
//!
//! Every external input (earnings-rs events, parquet rows, JSON files) is
//! translated here before it reaches business logic. This is the
//! Anti-Corruption Layer described in ADR-0001.
//!
//! Rule: no business logic in mappers, no provider-specific types outside mappers.

use crate::repositories::RepositoryError;

pub mod earnings;

/// Translate an external type into a canonical domain model.
///
/// Callers should `.into_normalized()` immediately after reading from any
/// provider-specific source, before passing the value to business logic.
pub trait IntoNormalized<T> {
    fn into_normalized(self) -> Result<T, RepositoryError>;
}
