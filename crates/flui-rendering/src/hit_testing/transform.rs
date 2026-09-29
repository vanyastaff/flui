//! Transform parts for hit test coordinate transformation.

use flui_foundation::geometry::{Matrix4, Offset};

/// A part of a transform that can be applied to or inverted for positions.
///
/// This is used to efficiently transform positions during hit testing
/// without having to compute full matrix inverses for simple operations.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `_TransformPart` and related classes.
#[derive(Debug, Clone)]
pub enum MatrixTransformPart {
    /// A simple offset translation.
    Offset(Offset),

    /// A full 4x4 matrix transformation.
    Matrix(Matrix4),
}

impl MatrixTransformPart {
    /// Creates an offset transform part.
    pub fn offset(dx: f64, dy: f64) -> Self {
        Self::Offset(Offset::new(dx, dy))
    }

    /// Creates a matrix transform part.
    pub fn matrix(m: Matrix4) -> Self {
        Self::Matrix(m)
    }

    /// Transforms a local position to parent coordinates.
    pub fn local_to_global(&self, position: Offset) -> Offset {
        match self {
            Self::Offset(offset) => Offset::new(position.dx + offset.dx, position.dy + offset.dy),
            Self::Matrix(m) => {
                let (x, y) = m.transform_point(position.dx, position.dy);
                Offset::new(x, y)
            }
        }
    }

    /// Transforms a global position to local coordinates.
    pub fn global_to_local(&self, position: Offset) -> Option<Offset> {
        match self {
            Self::Offset(offset) => Some(Offset::new(
                position.dx - offset.dx,
                position.dy - offset.dy,
            )),
            Self::Matrix(m) => m.try_inverse().map(|inverse| {
                let (x, y) = inverse.transform_point(position.dx, position.dy);
                Offset::new(x, y)
            }),
        }
    }

    /// Returns the equivalent matrix for this transform part.
    pub fn to_matrix(&self) -> Matrix4 {
        match self {
            Self::Offset(offset) => Matrix4::translation(offset.dx, offset.dy, 0.0),
            Self::Matrix(m) => *m,
        }
    }

    /// Returns true if this is an identity transform.
    pub fn is_identity(&self) -> bool {
        match self {
            Self::Offset(offset) => offset.dx == 0.0 && offset.dy == 0.0,
            Self::Matrix(m) => *m == Matrix4::IDENTITY,
        }
    }
}

impl Default for MatrixTransformPart {
    fn default() -> Self {
        Self::Offset(Offset::ZERO)
    }
}

impl From<Offset> for MatrixTransformPart {
    fn from(offset: Offset) -> Self {
        Self::Offset(offset)
    }
}

impl From<Matrix4> for MatrixTransformPart {
    fn from(matrix: Matrix4) -> Self {
        Self::Matrix(matrix)
    }
}

#[cfg(test)]
mod tests {}
