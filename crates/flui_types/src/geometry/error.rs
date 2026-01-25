/// Errors that can occur during geometry operations.
///
/// This enum provides comprehensive error types for validating geometric data,
/// detecting invalid operations, and handling edge cases in geometry computations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GeometryError {
    // ========================================================================
    // Coordinate/Value Errors
    // ========================================================================
    /// Invalid coordinate values (NaN or infinite).
    #[error("Invalid coordinates: ({x}, {y}) - must be finite")]
    InvalidCoordinates {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    /// Invalid numeric value (NaN or infinite).
    #[error("Invalid value: {value} - must be finite")]
    InvalidValue {
        /// The invalid value
        value: f32,
        /// Context about what the value represents
        context: &'static str,
    },

    /// Value outside the valid range.
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
    /// Negative dimension value where positive is required.
    #[error("Negative dimension: {dimension} = {value} (must be >= 0)")]
    NegativeDimension {
        /// Name of the dimension (e.g., "width", "height", "radius")
        dimension: &'static str,
        /// The invalid value
        value: f32,
    },

    /// Zero dimension value where non-zero is required.
    #[error("Zero dimension: {dimension} (must be non-zero)")]
    ZeroDimension {
        /// Name of the dimension
        dimension: &'static str,
    },

    /// Invalid size with both width and height.
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
    /// Attempted division by zero.
    #[error("Division by zero")]
    DivisionByZero,

    /// Arithmetic overflow occurred.
    #[error("Arithmetic overflow: {operation}")]
    Overflow {
        /// Description of the operation that overflowed
        operation: &'static str,
    },

    /// Arithmetic underflow occurred.
    #[error("Arithmetic underflow: {operation}")]
    Underflow {
        /// Description of the operation that underflowed
        operation: &'static str,
    },

    // ========================================================================
    // Geometric Constraint Errors
    // ========================================================================
    /// Attempted to normalize a zero-length vector.
    #[error("Cannot normalize zero-length vector")]
    ZeroLengthVector,

    /// Lines are parallel and have no intersection.
    #[error("Lines are parallel: no intersection point")]
    ParallelLines,

    /// Lines are collinear and have infinite intersections.
    #[error("Lines are collinear: infinite intersection points")]
    CollinearLines,

    /// Point is outside the valid bounds.
    #[error("Point ({x}, {y}) is outside bounds")]
    PointOutOfBounds {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    /// Degenerate geometric shape (zero area, collinear points, etc.).
    #[error("Degenerate geometry: {description}")]
    DegenerateGeometry {
        /// Description of the degenerate case
        description: &'static str,
    },

    // ========================================================================
    // Transform Errors
    // ========================================================================
    /// Matrix cannot be inverted (determinant is zero or near-zero).
    #[error("Matrix is not invertible (determinant is zero or near-zero)")]
    SingularMatrix,

    /// Invalid rotation angle value.
    #[error("Invalid rotation angle: {angle} radians")]
    InvalidAngle {
        /// The invalid angle value
        angle: f32,
    },

    /// Invalid scale factor.
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
    /// Curve parameter outside valid range.
    #[error("Invalid parameter t = {t} (expected range [{min}, {max}])")]
    InvalidParameter {
        /// The parameter value
        t: f32,
        /// Expected minimum
        min: f32,
        /// Expected maximum
        max: f32,
    },

    /// Not enough control points for the operation.
    #[error("Insufficient control points: have {have}, need at least {need}")]
    InsufficientControlPoints {
        /// Number of points provided
        have: usize,
        /// Minimum required
        need: usize,
    },

    /// Path contains no segments.
    #[error("Path is empty")]
    EmptyPath,

    /// Path is not closed when closure is required.
    #[error("Path is not closed")]
    PathNotClosed,

    // ========================================================================
    // Generic Errors
    // ========================================================================
    /// Generic invalid operation error.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Generic constraint violation error.
    #[error("Constraint violated: {0}")]
    ConstraintViolation(String),
}

impl GeometryError {
    // ========================================================================
    // Convenience constructors
    // ========================================================================

    /// Creates an invalid coordinates error.
    #[inline]
    pub fn invalid_coords(x: f32, y: f32) -> Self {
        Self::InvalidCoordinates { x, y }
    }

    /// Creates an invalid value error.
    #[inline]
    pub fn invalid_value(value: f32, context: &'static str) -> Self {
        Self::InvalidValue { value, context }
    }

    /// Creates an out of range error.
    #[inline]
    pub fn out_of_range(value: f32, min: f32, max: f32, context: &'static str) -> Self {
        Self::OutOfRange {
            value,
            min,
            max,
            context,
        }
    }

    /// Creates a negative dimension error.
    #[inline]
    pub fn negative_dimension(dimension: &'static str, value: f32) -> Self {
        Self::NegativeDimension { dimension, value }
    }

    /// Creates a zero dimension error.
    #[inline]
    pub fn zero_dimension(dimension: &'static str) -> Self {
        Self::ZeroDimension { dimension }
    }

    /// Creates an invalid size error.
    #[inline]
    pub fn invalid_size(width: f32, height: f32, reason: &'static str) -> Self {
        Self::InvalidSize {
            width,
            height,
            reason,
        }
    }

    /// Creates a degenerate geometry error.
    #[inline]
    pub fn degenerate(description: &'static str) -> Self {
        Self::DegenerateGeometry { description }
    }

    /// Creates an invalid parameter error for t in [0, 1].
    #[inline]
    pub fn invalid_t(t: f32) -> Self {
        Self::InvalidParameter {
            t,
            min: 0.0,
            max: 1.0,
        }
    }

    /// Creates an invalid parameter error with custom range.
    #[inline]
    pub fn invalid_param(t: f32, min: f32, max: f32) -> Self {
        Self::InvalidParameter { t, min, max }
    }

    /// Creates an invalid scale error.
    #[inline]
    pub fn invalid_scale(factor: f32, reason: &'static str) -> Self {
        Self::InvalidScale { factor, reason }
    }

    /// Creates a point out of bounds error.
    #[inline]
    pub fn point_out_of_bounds(x: f32, y: f32) -> Self {
        Self::PointOutOfBounds { x, y }
    }

    // ========================================================================
    // Predicates
    // ========================================================================

    /// Returns `true` if this is a value validity error (NaN, infinite, or out of range).
    #[inline]
    pub fn is_validity_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidCoordinates { .. } | Self::InvalidValue { .. } | Self::OutOfRange { .. }
        )
    }

    /// Returns `true` if this is a dimension error (negative, zero, or invalid size).
    #[inline]
    pub fn is_dimension_error(&self) -> bool {
        matches!(
            self,
            Self::NegativeDimension { .. } | Self::ZeroDimension { .. } | Self::InvalidSize { .. }
        )
    }

    /// Returns `true` if this is an arithmetic error (division by zero, overflow, underflow).
    #[inline]
    pub fn is_arithmetic_error(&self) -> bool {
        matches!(
            self,
            Self::DivisionByZero | Self::Overflow { .. } | Self::Underflow { .. }
        )
    }

    /// Returns `true` if this is a geometric constraint error.
    #[inline]
    pub fn is_geometric_error(&self) -> bool {
        matches!(
            self,
            Self::ZeroLengthVector
                | Self::ParallelLines
                | Self::CollinearLines
                | Self::PointOutOfBounds { .. }
                | Self::DegenerateGeometry { .. }
        )
    }

    /// Returns `true` if this is a transformation error.
    #[inline]
    pub fn is_transform_error(&self) -> bool {
        matches!(
            self,
            Self::SingularMatrix | Self::InvalidAngle { .. } | Self::InvalidScale { .. }
        )
    }

    /// Returns `true` if this is a path or curve error.
    #[inline]
    pub fn is_path_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidParameter { .. }
                | Self::InsufficientControlPoints { .. }
                | Self::EmptyPath
                | Self::PathNotClosed
        )
    }
}

/// Result type alias for geometry operations.
pub type GeometryResult<T> = Result<T, GeometryError>;

#[cfg(test)]
mod tests {
    use super::*;
}
