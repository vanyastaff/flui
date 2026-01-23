
#[derive(Debug, Clone, thiserror::Error)]
pub enum GeometryError {
    // ========================================================================
    // Coordinate/Value Errors
    // ========================================================================

    #[error("Invalid coordinates: ({x}, {y}) - must be finite")]
    InvalidCoordinates {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    #[error("Invalid value: {value} - must be finite")]
    InvalidValue {
        /// The invalid value
        value: f32,
        /// Context about what the value represents
        context: &'static str,
    },

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

    #[error("Negative dimension: {dimension} = {value} (must be >= 0)")]
    NegativeDimension {
        /// Name of the dimension (e.g., "width", "height", "radius")
        dimension: &'static str,
        /// The invalid value
        value: f32,
    },

    #[error("Zero dimension: {dimension} (must be non-zero)")]
    ZeroDimension {
        /// Name of the dimension
        dimension: &'static str,
    },

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

    #[error("Division by zero")]
    DivisionByZero,

    #[error("Arithmetic overflow: {operation}")]
    Overflow {
        /// Description of the operation that overflowed
        operation: &'static str,
    },

    #[error("Arithmetic underflow: {operation}")]
    Underflow {
        /// Description of the operation that underflowed
        operation: &'static str,
    },

    // ========================================================================
    // Geometric Constraint Errors
    // ========================================================================

    #[error("Cannot normalize zero-length vector")]
    ZeroLengthVector,

    #[error("Lines are parallel: no intersection point")]
    ParallelLines,

    #[error("Lines are collinear: infinite intersection points")]
    CollinearLines,

    #[error("Point ({x}, {y}) is outside bounds")]
    PointOutOfBounds {
        /// X coordinate
        x: f32,
        /// Y coordinate
        y: f32,
    },

    #[error("Degenerate geometry: {description}")]
    DegenerateGeometry {
        /// Description of the degenerate case
        description: &'static str,
    },

    // ========================================================================
    // Transform Errors
    // ========================================================================

    #[error("Matrix is not invertible (determinant is zero or near-zero)")]
    SingularMatrix,

    #[error("Invalid rotation angle: {angle} radians")]
    InvalidAngle {
        /// The invalid angle value
        angle: f32,
    },

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

    #[error("Invalid parameter t = {t} (expected range [{min}, {max}])")]
    InvalidParameter {
        /// The parameter value
        t: f32,
        /// Expected minimum
        min: f32,
        /// Expected maximum
        max: f32,
    },

    #[error("Insufficient control points: have {have}, need at least {need}")]
    InsufficientControlPoints {
        /// Number of points provided
        have: usize,
        /// Minimum required
        need: usize,
    },

    #[error("Path is empty")]
    EmptyPath,

    #[error("Path is not closed")]
    PathNotClosed,

    // ========================================================================
    // Generic Errors
    // ========================================================================

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Constraint violated: {0}")]
    ConstraintViolation(String),
}

impl GeometryError {
    // ========================================================================
    // Convenience constructors
    // ========================================================================

    #[inline]
    pub fn invalid_coords(x: f32, y: f32) -> Self {
        Self::InvalidCoordinates { x, y }
    }

    #[inline]
    pub fn invalid_value(value: f32, context: &'static str) -> Self {
        Self::InvalidValue { value, context }
    }

    #[inline]
    pub fn out_of_range(value: f32, min: f32, max: f32, context: &'static str) -> Self {
        Self::OutOfRange { value, min, max, context }
    }

    #[inline]
    pub fn negative_dimension(dimension: &'static str, value: f32) -> Self {
        Self::NegativeDimension { dimension, value }
    }

    #[inline]
    pub fn zero_dimension(dimension: &'static str) -> Self {
        Self::ZeroDimension { dimension }
    }

    #[inline]
    pub fn invalid_size(width: f32, height: f32, reason: &'static str) -> Self {
        Self::InvalidSize { width, height, reason }
    }

    #[inline]
    pub fn degenerate(description: &'static str) -> Self {
        Self::DegenerateGeometry { description }
    }

    #[inline]
    pub fn invalid_t(t: f32) -> Self {
        Self::InvalidParameter { t, min: 0.0, max: 1.0 }
    }

    #[inline]
    pub fn invalid_param(t: f32, min: f32, max: f32) -> Self {
        Self::InvalidParameter { t, min, max }
    }

    #[inline]
    pub fn invalid_scale(factor: f32, reason: &'static str) -> Self {
        Self::InvalidScale { factor, reason }
    }

    #[inline]
    pub fn point_out_of_bounds(x: f32, y: f32) -> Self {
        Self::PointOutOfBounds { x, y }
    }

    // ========================================================================
    // Predicates
    // ========================================================================

    #[inline]
    pub fn is_validity_error(&self) -> bool {
        matches!(self,
            Self::InvalidCoordinates { .. } |
            Self::InvalidValue { .. } |
            Self::OutOfRange { .. }
        )
    }

    #[inline]
    pub fn is_dimension_error(&self) -> bool {
        matches!(self,
            Self::NegativeDimension { .. } |
            Self::ZeroDimension { .. } |
            Self::InvalidSize { .. }
        )
    }

    #[inline]
    pub fn is_arithmetic_error(&self) -> bool {
        matches!(self,
            Self::DivisionByZero |
            Self::Overflow { .. } |
            Self::Underflow { .. }
        )
    }

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

    #[inline]
    pub fn is_transform_error(&self) -> bool {
        matches!(self,
            Self::SingularMatrix |
            Self::InvalidAngle { .. } |
            Self::InvalidScale { .. }
        )
    }

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

}
