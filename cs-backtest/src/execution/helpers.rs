//! Helper functions for trade execution
//!
//! Extracts common patterns across different trade implementations to reduce boilerplate.

use std::future::Future;
use cs_domain::FailureReason;
use super::types::ExecutionError;

/// Run a batch of async operations, either in parallel or sequentially.
///
/// This abstracts the common pattern of:
/// ```ignore
/// if parallel {
///     futures::future::join_all(items.iter().map(f)).await
/// } else {
///     for item in items { f(item).await }
/// }
/// ```
///
/// # Arguments
/// * `items` - Slice of items to process
/// * `parallel` - If true, run all futures concurrently; if false, run sequentially
/// * `f` - Closure that produces a future for each item
///
/// # Returns
/// Vector of results in the same order as input items
pub async fn run_batch<'a, T: 'a, F, Fut, R>(
    items: &'a [T],
    parallel: bool,
    f: F,
) -> Vec<R>
where
    F: Fn(&'a T) -> Fut,
    Fut: Future<Output = R> + Send,
    R: Send,
{
    if parallel {
        let futures: Vec<_> = items.iter().map(&f).collect();
        futures::future::join_all(futures).await
    } else {
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            results.push(f(item).await);
        }
        results
    }
}

/// Map ExecutionError to a domain FailureReason
///
/// This mapping is consistent across all trade types and is used in to_failed_result()
/// implementations to convert execution errors into user-friendly failure reasons.
pub fn error_to_failure_reason(error: &ExecutionError) -> FailureReason {
    match error {
        ExecutionError::NoSpotPrice => FailureReason::NoSpotPrice,
        ExecutionError::Repository(_) => FailureReason::NoOptionsData,
        ExecutionError::Pricing(_) => FailureReason::PricingError(error.to_string()),
        ExecutionError::InvalidSpread(_) => FailureReason::DegenerateSpread,
    }
}
