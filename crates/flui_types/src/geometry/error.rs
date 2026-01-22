//! Error types for geometry operations.

/// Errors that can occur during geometry operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GeometryError {
    /// Coordinates are not finite (NaN or infinity)
    #[error("Invalid coordinates: ({x}, {y}) - must be finite")]
    InvalidCoordinates {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    /// Division by zero attempted
    #[error("Division by zero")]
    DivisionByZero,

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = GeometryError::InvalidCoordinates {
            x: f32::NAN,
            y: 100.0,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid coordinates"));
        assert!(msg.contains("must be finite"));
    }

    #[test]
    fn test_error_debug() {
        let err = GeometryError::DivisionByZero;
        let msg = format!("{:?}", err);
        assert!(msg.contains("DivisionByZero"));
    }

    #[test]
    fn test_invalid_operation() {
        let err = GeometryError::InvalidOperation("test error".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid operation"));
        assert!(msg.contains("test error"));
    }
}
