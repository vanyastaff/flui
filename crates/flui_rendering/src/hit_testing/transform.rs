//! Transform parts for hit test coordinate transformation.

use flui_types::{Matrix4, Offset};

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
    pub fn offset(dx: f32, dy: f32) -> Self {
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
mod tests {
    use super::*;

    #[test]
    fn test_offset_local_to_global() {
        let transform = MatrixTransformPart::offset(10.0, 20.0);
        let local = Offset::new(5.0, 5.0);
        let global = transform.local_to_global(local);
        assert_eq!(global.dx, 15.0);
        assert_eq!(global.dy, 25.0);
    }

    #[test]
    fn test_offset_global_to_local() {
        let transform = MatrixTransformPart::offset(10.0, 20.0);
        let global = Offset::new(15.0, 25.0);
        let local = transform.global_to_local(global).unwrap();
        assert_eq!(local.dx, 5.0);
        assert_eq!(local.dy, 5.0);
    }

    #[test]
    fn test_matrix_translation() {
        let m = Matrix4::translation(10.0, 20.0, 0.0);
        let transform = MatrixTransformPart::Matrix(m);
        let local = Offset::new(5.0, 5.0);
        let global = transform.local_to_global(local);

        assert!((global.dx - 15.0).abs() < 0.001);
        assert!((global.dy - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_matrix_scale() {
        let m = Matrix4::scaling(2.0, 2.0, 1.0);
        let transform = MatrixTransformPart::Matrix(m);
        let local = Offset::new(5.0, 10.0);
        let global = transform.local_to_global(local);

        assert!((global.dx - 10.0).abs() < 0.001);
        assert!((global.dy - 20.0).abs() < 0.001);

        let back = transform.global_to_local(global).unwrap();
        assert!((back.dx - 5.0).abs() < 0.001);
        assert!((back.dy - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_is_identity() {
        assert!(MatrixTransformPart::offset(0.0, 0.0).is_identity());
        assert!(!MatrixTransformPart::offset(1.0, 0.0).is_identity());
        assert!(MatrixTransformPart::Matrix(Matrix4::IDENTITY).is_identity());
    }

    #[test]
    fn test_to_matrix() {
        let transform = MatrixTransformPart::offset(10.0, 20.0);
        let matrix = transform.to_matrix();

        let (x, y) = matrix.transform_point(5.0, 5.0);
        assert!((x - 15.0).abs() < 0.001);
        assert!((y - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_from_offset() {
        let offset = Offset::new(10.0, 20.0);
        let transform: MatrixTransformPart = offset.into();

        if let MatrixTransformPart::Offset(o) = transform {
            assert_eq!(o.dx, 10.0);
            assert_eq!(o.dy, 20.0);
        } else {
            panic!("Expected Offset variant");
        }
    }
}
