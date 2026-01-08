//! Helper functions for trade execution
//!
//! Extracts common patterns across different trade implementations to reduce boilerplate.

use cs_domain::FailureReason;
use super::types::ExecutionError;

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
