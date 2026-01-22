//! Error types for geometry operations.

/// Errors that can occur during geometry operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GeometryError {
    // ========================================================================
    // Coordinate/Value Errors
    // ========================================================================

    /// Coordinates are not finite (NaN or infinity)
    #[error("Invalid coordinates: ({x}, {y}) - must be finite")]
    InvalidCoordinates {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    /// A single value is not finite (NaN or infinity)
    #[error("Invalid value: {value} - must be finite")]
    InvalidValue {
        /// The invalid value
        value: f32,
        /// Context about what the value represents
        context: &'static str,
    },

    /// Value is out of expected range
    #[error("{context}: {value} is out of range [{min}, {max}]")]
    OutOfRange {
        /// The invalid value
        value: f32,
        /// Minimum allowed value
        min: f32,
        /// Maximum allowed value
        max: f32,
        /// Context about what the value represents
        context: &'static str,
    },

    // ========================================================================
    // Dimension Errors
    // ========================================================================

    /// Negative dimension where positive is required
    #[error("Negative dimension: {dimension} = {value} (must be >= 0)")]
    NegativeDimension {
        /// Name of the dimension (e.g., "width", "height", "radius")
        dimension: &'static str,
        /// The invalid value
        value: f32,
    },

    /// Zero dimension where non-zero is required
    #[error("Zero dimension: {dimension} (must be non-zero)")]
    ZeroDimension {
        /// Name of the dimension
        dimension: &'static str,
    },

    /// Invalid size (width or height issues)
    #[error("Invalid size: {width}×{height} - {reason}")]
    InvalidSize {
        /// Width value
        width: f32,
        /// Height value
        height: f32,
        /// Reason for invalidity
        reason: &'static str,
    },

    // ========================================================================
    // Arithmetic Errors
    // ========================================================================

    /// Division by zero attempted
    #[error("Division by zero")]
    DivisionByZero,

    /// Arithmetic overflow
    #[error("Arithmetic overflow: {operation}")]
    Overflow {
        /// Description of the operation that overflowed
        operation: &'static str,
    },

    /// Arithmetic underflow
    #[error("Arithmetic underflow: {operation}")]
    Underflow {
        /// Description of the operation that underflowed
        operation: &'static str,
    },

    // ========================================================================
    // Geometric Constraint Errors
    // ========================================================================

    /// Failed to normalize a zero-length vector
    #[error("Cannot normalize zero-length vector")]
    ZeroLengthVector,

    /// Lines are parallel (no intersection)
    #[error("Lines are parallel: no intersection point")]
    ParallelLines,

    /// Lines are collinear (infinite intersections)
    #[error("Lines are collinear: infinite intersection points")]
    CollinearLines,

    /// Point is outside expected bounds
    #[error("Point ({x}, {y}) is outside bounds")]
    PointOutOfBounds {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    /// Empty or degenerate geometry
    #[error("Degenerate geometry: {description}")]
    DegenerateGeometry {
        /// Description of the degenerate case
        description: &'static str,
    },

    // ========================================================================
    // Transform Errors
    // ========================================================================

    /// Matrix is not invertible (singular matrix)
    #[error("Matrix is not invertible (determinant is zero or near-zero)")]
    SingularMatrix,

    /// Invalid rotation angle
    #[error("Invalid rotation angle: {angle} radians")]
    InvalidAngle {
        /// The invalid angle value
        angle: f32,
    },

    /// Invalid scale factor
    #[error("Invalid scale factor: {factor} - {reason}")]
    InvalidScale {
        /// The invalid scale factor
        factor: f32,
        /// Reason for invalidity
        reason: &'static str,
    },

    // ========================================================================
    // Path/Curve Errors
    // ========================================================================

    /// Invalid parameter t (should be in [0, 1] for most curves)
    #[error("Invalid parameter t = {t} (expected range [{min}, {max}])")]
    InvalidParameter {
        /// The parameter value
        t: f32,
        /// Expected minimum
        min: f32,
        /// Expected maximum
        max: f32,
    },

    /// Insufficient control points for curve
    #[error("Insufficient control points: have {have}, need at least {need}")]
    InsufficientControlPoints {
        /// Number of points provided
        have: usize,
        /// Minimum required
        need: usize,
    },

    /// Path is empty
    #[error("Path is empty")]
    EmptyPath,

    /// Path is not closed when closure is required
    #[error("Path is not closed")]
    PathNotClosed,

    // ========================================================================
    // Generic Errors
    // ========================================================================

    /// Invalid operation with custom message
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Constraint violation
    #[error("Constraint violated: {0}")]
    ConstraintViolation(String),
}

impl GeometryError {
    // ========================================================================
    // Convenience constructors
    // ========================================================================

    /// Creates an `InvalidCoordinates` error.
    #[inline]
    pub fn invalid_coords(x: f32, y: f32) -> Self {
        Self::InvalidCoordinates { x, y }
    }

    /// Creates an `InvalidValue` error.
    #[inline]
    pub fn invalid_value(value: f32, context: &'static str) -> Self {
        Self::InvalidValue { value, context }
    }

    /// Creates an `OutOfRange` error.
    #[inline]
    pub fn out_of_range(value: f32, min: f32, max: f32, context: &'static str) -> Self {
        Self::OutOfRange { value, min, max, context }
    }

    /// Creates a `NegativeDimension` error.
    #[inline]
    pub fn negative_dimension(dimension: &'static str, value: f32) -> Self {
        Self::NegativeDimension { dimension, value }
    }

    /// Creates a `ZeroDimension` error.
    #[inline]
    pub fn zero_dimension(dimension: &'static str) -> Self {
        Self::ZeroDimension { dimension }
    }

    /// Creates an `InvalidSize` error.
    #[inline]
    pub fn invalid_size(width: f32, height: f32, reason: &'static str) -> Self {
        Self::InvalidSize { width, height, reason }
    }

    /// Creates a `DegenerateGeometry` error.
    #[inline]
    pub fn degenerate(description: &'static str) -> Self {
        Self::DegenerateGeometry { description }
    }

    /// Creates an `InvalidParameter` error for t in [0, 1].
    #[inline]
    pub fn invalid_t(t: f32) -> Self {
        Self::InvalidParameter { t, min: 0.0, max: 1.0 }
    }

    /// Creates an `InvalidParameter` error with custom range.
    #[inline]
    pub fn invalid_param(t: f32, min: f32, max: f32) -> Self {
        Self::InvalidParameter { t, min, max }
    }

    /// Creates an `InvalidScale` error.
    #[inline]
    pub fn invalid_scale(factor: f32, reason: &'static str) -> Self {
        Self::InvalidScale { factor, reason }
    }

    /// Creates a `PointOutOfBounds` error.
    #[inline]
    pub fn point_out_of_bounds(x: f32, y: f32) -> Self {
        Self::PointOutOfBounds { x, y }
    }

    // ========================================================================
    // Predicates
    // ========================================================================

    /// Returns `true` if this is a coordinate/value validity error.
    #[inline]
    pub fn is_validity_error(&self) -> bool {
        matches!(self,
            Self::InvalidCoordinates { .. } |
            Self::InvalidValue { .. } |
            Self::OutOfRange { .. }
        )
    }

    /// Returns `true` if this is a dimension error.
    #[inline]
    pub fn is_dimension_error(&self) -> bool {
        matches!(self,
            Self::NegativeDimension { .. } |
            Self::ZeroDimension { .. } |
            Self::InvalidSize { .. }
        )
    }

    /// Returns `true` if this is an arithmetic error.
    #[inline]
    pub fn is_arithmetic_error(&self) -> bool {
        matches!(self,
            Self::DivisionByZero |
            Self::Overflow { .. } |
            Self::Underflow { .. }
        )
    }

    /// Returns `true` if this is a geometric constraint error.
    #[inline]
    pub fn is_geometric_error(&self) -> bool {
        matches!(self,
            Self::ZeroLengthVector |
            Self::ParallelLines |
            Self::CollinearLines |
            Self::PointOutOfBounds { .. } |
            Self::DegenerateGeometry { .. }
        )
    }

    /// Returns `true` if this is a transform error.
    #[inline]
    pub fn is_transform_error(&self) -> bool {
        matches!(self,
            Self::SingularMatrix |
            Self::InvalidAngle { .. } |
            Self::InvalidScale { .. }
        )
    }

    /// Returns `true` if this is a path/curve error.
    #[inline]
    pub fn is_path_error(&self) -> bool {
        matches!(self,
            Self::InvalidParameter { .. } |
            Self::InsufficientControlPoints { .. } |
            Self::EmptyPath |
            Self::PathNotClosed
        )
    }
}

/// Result type alias for geometry operations.
pub type GeometryResult<T> = Result<T, GeometryError>;

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

    #[test]
    fn test_convenience_constructors() {
        let err = GeometryError::invalid_coords(f32::NAN, 10.0);
        assert!(matches!(err, GeometryError::InvalidCoordinates { .. }));

        let err = GeometryError::invalid_value(f32::INFINITY, "radius");
        assert!(matches!(err, GeometryError::InvalidValue { .. }));

        let err = GeometryError::out_of_range(1.5, 0.0, 1.0, "parameter t");
        assert!(matches!(err, GeometryError::OutOfRange { .. }));

        let err = GeometryError::negative_dimension("width", -10.0);
        assert!(matches!(err, GeometryError::NegativeDimension { .. }));

        let err = GeometryError::zero_dimension("radius");
        assert!(matches!(err, GeometryError::ZeroDimension { .. }));

        let err = GeometryError::invalid_size(0.0, 100.0, "width must be positive");
        assert!(matches!(err, GeometryError::InvalidSize { .. }));

        let err = GeometryError::degenerate("line has zero length");
        assert!(matches!(err, GeometryError::DegenerateGeometry { .. }));

        let err = GeometryError::invalid_t(1.5);
        assert!(matches!(err, GeometryError::InvalidParameter { t, min, max } if t == 1.5 && min == 0.0 && max == 1.0));

        let err = GeometryError::invalid_scale(0.0, "cannot be zero");
        assert!(matches!(err, GeometryError::InvalidScale { .. }));

        let err = GeometryError::point_out_of_bounds(150.0, 200.0);
        assert!(matches!(err, GeometryError::PointOutOfBounds { .. }));
    }

    #[test]
    fn test_error_predicates() {
        // Validity errors
        assert!(GeometryError::invalid_coords(f32::NAN, 0.0).is_validity_error());
        assert!(GeometryError::invalid_value(f32::INFINITY, "x").is_validity_error());
        assert!(GeometryError::out_of_range(1.5, 0.0, 1.0, "t").is_validity_error());

        // Dimension errors
        assert!(GeometryError::negative_dimension("width", -1.0).is_dimension_error());
        assert!(GeometryError::zero_dimension("height").is_dimension_error());
        assert!(GeometryError::invalid_size(0.0, 0.0, "empty").is_dimension_error());

        // Arithmetic errors
        assert!(GeometryError::DivisionByZero.is_arithmetic_error());
        assert!(GeometryError::Overflow { operation: "mul" }.is_arithmetic_error());
        assert!(GeometryError::Underflow { operation: "sub" }.is_arithmetic_error());

        // Geometric errors
        assert!(GeometryError::ZeroLengthVector.is_geometric_error());
        assert!(GeometryError::ParallelLines.is_geometric_error());
        assert!(GeometryError::CollinearLines.is_geometric_error());
        assert!(GeometryError::point_out_of_bounds(0.0, 0.0).is_geometric_error());
        assert!(GeometryError::degenerate("empty").is_geometric_error());

        // Transform errors
        assert!(GeometryError::SingularMatrix.is_transform_error());
        assert!(GeometryError::InvalidAngle { angle: f32::NAN }.is_transform_error());
        assert!(GeometryError::invalid_scale(0.0, "zero").is_transform_error());

        // Path errors
        assert!(GeometryError::invalid_t(1.5).is_path_error());
        assert!(GeometryError::InsufficientControlPoints { have: 2, need: 3 }.is_path_error());
        assert!(GeometryError::EmptyPath.is_path_error());
        assert!(GeometryError::PathNotClosed.is_path_error());
    }

    #[test]
    fn test_geometry_result_type() {
        fn test_func(valid: bool) -> GeometryResult<f32> {
            if valid {
                Ok(42.0)
            } else {
                Err(GeometryError::DivisionByZero)
            }
        }

        assert!(test_func(true).is_ok());
        assert!(test_func(false).is_err());
    }

    #[test]
    fn test_error_messages() {
        // Test that error messages are informative
        let err = GeometryError::OutOfRange {
            value: 1.5,
            min: 0.0,
            max: 1.0,
            context: "parameter t",
        };
        let msg = format!("{}", err);
        assert!(msg.contains("1.5"));
        assert!(msg.contains("[0, 1]"));
        assert!(msg.contains("parameter t"));

        let err = GeometryError::NegativeDimension {
            dimension: "width",
            value: -10.0,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("width"));
        assert!(msg.contains("-10"));

        let err = GeometryError::InsufficientControlPoints {
            have: 2,
            need: 4,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("have 2"));
        assert!(msg.contains("need at least 4"));
    }
}
