//! Validation types for form inputs
//!
//! This module contains types for representing validation states and errors.

/// Validation state for form fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidationState {
    /// Not yet validated
    Pending,
    /// Currently validating (async validation)
    Validating,
    /// Validation passed
    Valid,
    /// Validation failed
    Invalid,
}

impl ValidationState {
    /// Check if validation is complete.
    pub fn is_complete(&self) -> bool {
        matches!(self, ValidationState::Valid | ValidationState::Invalid)
    }

    /// Check if validation passed.
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationState::Valid)
    }

    /// Check if validation failed.
    pub fn is_invalid(&self) -> bool {
        matches!(self, ValidationState::Invalid)
    }

    /// Check if validation is in progress.
    pub fn is_validating(&self) -> bool {
        matches!(self, ValidationState::Validating)
    }
}

impl Default for ValidationState {
    fn default() -> Self {
        ValidationState::Pending
    }
}

/// Validation error with message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Error message
    pub message: String,
    /// Error code (optional, for i18n)
    pub code: Option<String>,
}

impl ValidationError {
    /// Create a new validation error with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    /// Create a validation error with message and code.
    pub fn with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code.into()),
        }
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get the error code if present.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Validation result type.
pub type ValidationResult = Result<(), ValidationError>;

/// Common validation errors.
impl ValidationError {
    /// Required field error.
    pub fn required() -> Self {
        Self::with_code("This field is required", "required")
    }

    /// Invalid format error.
    pub fn invalid_format(field_type: &str) -> Self {
        Self::with_code(
            format!("Invalid {} format", field_type),
            "invalid_format",
        )
    }

    /// Minimum length error.
    pub fn min_length(min: usize) -> Self {
        Self::with_code(
            format!("Must be at least {} characters", min),
            "min_length",
        )
    }

    /// Maximum length error.
    pub fn max_length(max: usize) -> Self {
        Self::with_code(
            format!("Must be at most {} characters", max),
            "max_length",
        )
    }

    /// Minimum value error.
    pub fn min_value(min: f64) -> Self {
        Self::with_code(format!("Must be at least {}", min), "min_value")
    }

    /// Maximum value error.
    pub fn max_value(max: f64) -> Self {
        Self::with_code(format!("Must be at most {}", max), "max_value")
    }

    /// Pattern mismatch error.
    pub fn pattern_mismatch() -> Self {
        Self::with_code("Does not match required pattern", "pattern_mismatch")
    }

    /// Email format error.
    pub fn invalid_email() -> Self {
        Self::with_code("Invalid email address", "invalid_email")
    }

    /// URL format error.
    pub fn invalid_url() -> Self {
        Self::with_code("Invalid URL", "invalid_url")
    }

    /// Custom error with message.
    pub fn custom(message: impl Into<String>) -> Self {
        Self::new(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_state() {
        assert!(!ValidationState::Pending.is_complete());
        assert!(!ValidationState::Validating.is_complete());
        assert!(ValidationState::Valid.is_complete());
        assert!(ValidationState::Invalid.is_complete());

        assert!(ValidationState::Valid.is_valid());
        assert!(!ValidationState::Invalid.is_valid());

        assert!(ValidationState::Invalid.is_invalid());
        assert!(!ValidationState::Valid.is_invalid());

        assert!(ValidationState::Validating.is_validating());
        assert!(!ValidationState::Pending.is_validating());
    }

    #[test]
    fn test_validation_error_creation() {
        let error = ValidationError::new("Test error");
        assert_eq!(error.message(), "Test error");
        assert_eq!(error.code(), None);

        let error_with_code = ValidationError::with_code("Test error", "test_code");
        assert_eq!(error_with_code.message(), "Test error");
        assert_eq!(error_with_code.code(), Some("test_code"));
    }

    #[test]
    fn test_common_validation_errors() {
        let required = ValidationError::required();
        assert_eq!(required.code(), Some("required"));

        let min_length = ValidationError::min_length(5);
        assert!(min_length.message().contains("5"));

        let max_length = ValidationError::max_length(10);
        assert!(max_length.message().contains("10"));

        let invalid_email = ValidationError::invalid_email();
        assert_eq!(invalid_email.code(), Some("invalid_email"));

        let custom = ValidationError::custom("Custom error");
        assert_eq!(custom.message(), "Custom error");
        assert_eq!(custom.code(), None);
    }

    #[test]
    fn test_validation_error_display() {
        let error = ValidationError::new("Test error");
        assert_eq!(format!("{}", error), "Test error");
    }

    #[test]
    fn test_validation_result() {
        let success: ValidationResult = Ok(());
        assert!(success.is_ok());

        let failure: ValidationResult = Err(ValidationError::required());
        assert!(failure.is_err());
    }

    #[test]
    fn test_validation_state_default() {
        assert_eq!(ValidationState::default(), ValidationState::Pending);
    }

    #[test]
    fn test_number_validation_errors() {
        let min_val = ValidationError::min_value(0.0);
        assert!(min_val.message().contains("0"));
        assert_eq!(min_val.code(), Some("min_value"));

        let max_val = ValidationError::max_value(100.0);
        assert!(max_val.message().contains("100"));
        assert_eq!(max_val.code(), Some("max_value"));
    }
}
