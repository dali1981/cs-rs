//! Centralized IV validation with configurable bounds

/// IV validation result
#[derive(Debug, Clone, Copy)]
pub enum IVValidationError {
    TooLow(f64),
    TooHigh(f64),
}

/// Central IV validator with configurable bounds
#[derive(Debug, Clone, Copy)]
pub struct IVValidator {
    pub min_iv: f64,
    pub max_iv: f64,
}

impl IVValidator {
    /// Create validator with custom bounds
    pub fn with_bounds(min_iv: f64, max_iv: f64) -> Self {
        Self { min_iv, max_iv }
    }

    /// Create validator with default bounds (0.01 to 5.0)
    pub fn default() -> Self {
        Self {
            min_iv: 0.01,
            max_iv: 5.0,
        }
    }

    /// Validate a single IV value
    pub fn validate(&self, iv: f64) -> Result<f64, IVValidationError> {
        if iv < self.min_iv {
            Err(IVValidationError::TooLow(iv))
        } else if iv > self.max_iv {
            Err(IVValidationError::TooHigh(iv))
        } else {
            Ok(iv)
        }
    }

    /// Check if IV is valid without error details
    pub fn is_valid(&self, iv: f64) -> bool {
        iv >= self.min_iv && iv <= self.max_iv
    }

    /// Filter IVs to only valid ones
    pub fn filter_valid(&self, ivs: Vec<f64>) -> Vec<f64> {
        ivs.into_iter().filter(|&iv| self.is_valid(iv)).collect()
    }
}

impl Default for IVValidator {
    fn default() -> Self {
        Self::default()
    }
}

/// Validate IV for surface construction (accepts values in bounds)
pub fn validate_iv_for_surface(iv: f64) -> bool {
    IVValidator::default().is_valid(iv)
}

/// Validate entry IV against optional maximum threshold
pub fn validate_entry_iv(iv: Option<f64>, max_iv_threshold: Option<f64>) -> bool {
    match (iv, max_iv_threshold) {
        (Some(iv_val), Some(max)) => iv_val <= max,
        (None, _) => true,
        (Some(_), None) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_bounds() {
        let validator = IVValidator::default();
        assert_eq!(validator.min_iv, 0.01);
        assert_eq!(validator.max_iv, 5.0);
    }

    #[test]
    fn test_validate_valid_iv() {
        let validator = IVValidator::default();
        assert!(validator.validate(0.25).is_ok());
        assert!(validator.validate(1.0).is_ok());
        assert!(validator.validate(2.5).is_ok());
    }

    #[test]
    fn test_validate_invalid_iv() {
        let validator = IVValidator::default();
        assert!(validator.validate(0.005).is_err());
        assert!(validator.validate(6.0).is_err());
    }

    #[test]
    fn test_is_valid() {
        let validator = IVValidator::default();
        assert!(validator.is_valid(0.25));
        assert!(!validator.is_valid(0.005));
        assert!(!validator.is_valid(6.0));
    }

    #[test]
    fn test_filter_valid() {
        let validator = IVValidator::default();
        let ivs = vec![0.005, 0.25, 1.0, 6.0];
        let filtered = validator.filter_valid(ivs);
        assert_eq!(filtered, vec![0.25, 1.0]);
    }

    #[test]
    fn test_custom_bounds() {
        let validator = IVValidator::with_bounds(0.1, 2.0);
        assert!(validator.is_valid(0.5));
        assert!(!validator.is_valid(0.05));
        assert!(!validator.is_valid(3.0));
    }
}
