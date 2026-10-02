//! Immutable geometry admission before clip recording; no GPU allocation.
use crate::error::GeometryError;
use flui_foundation::geometry::{Point, RRect, RSuperellipse, Radius, Rect};
use flui_painting::paint::{Path, PathCommand};

/// Conservative payload bound, not a promise of subpixel accuracy at this limit.
pub(crate) const GPU_CLIP_LIMIT: f64 = 1_048_576.0;

pub(crate) fn pack_clip_value(value: f64, context: &'static str) -> Result<f32, GeometryError> {
    let packed = value as f32;
    if !value.is_finite()
        || value.abs() > GPU_CLIP_LIMIT
        || !packed.is_finite()
        || (value != 0.0 && packed == 0.0)
    {
        Err(GeometryError::Unrepresentable { context })
    } else {
        Ok(packed)
    }
}

/// A finite 2D affine transform, with singular transforms explicitly empty.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValidatedAffine {
    matrix: glam::DAffine2,
    inverse: Option<glam::DAffine2>,
}

impl ValidatedAffine {
    pub(crate) fn new(matrix: glam::DMat4) -> Result<Self, GeometryError> {
        if !matrix.is_finite() {
            return Err(GeometryError::NonFinite {
                context: "transform",
            });
        }
        let coefficients = matrix.to_cols_array();
        if coefficients[2] != 0.0
            || coefficients[3] != 0.0
            || coefficients[6] != 0.0
            || coefficients[7] != 0.0
            || coefficients[8] != 0.0
            || coefficients[9] != 0.0
            || coefficients[10] != 1.0
            || coefficients[11] != 0.0
            || coefficients[14] != 0.0
            || coefficients[15] != 1.0
        {
            return Err(GeometryError::UnsupportedTransform);
        }
        // Scale before testing rank: a tiny uniform scale is not singular
        // merely because its unnormalized determinant underflows.
        let maximum = coefficients[0]
            .abs()
            .max(coefficients[1].abs())
            .max(coefficients[4].abs())
            .max(coefficients[5].abs());
        // A power-of-two divisor preserves binary significands. Arbitrary
        // normalization can round distinct columns into proportional ones.
        let scale = if maximum == 0.0 {
            0.0
        } else {
            f64::from_bits(maximum.to_bits() & 0x7ff0_0000_0000_0000).max(f64::MIN_POSITIVE)
        };
        let mut normalized = coefficients;
        if scale > 0.0 {
            for index in [0, 1, 4, 5] {
                normalized[index] /= scale;
                if coefficients[index] != 0.0 && normalized[index] == 0.0 {
                    return Err(GeometryError::NonFiniteInverse);
                }
            }
        }
        let determinant = glam::DMat2::from_cols(
            glam::dvec2(normalized[0], normalized[1]),
            glam::dvec2(normalized[4], normalized[5]),
        )
        .determinant();
        let singular = if scale == 0.0 {
            true
        } else if determinant == 0.0 {
            let a = normalized[0];
            let b = normalized[1];
            let c = normalized[4];
            let d = normalized[5];
            let left = a * d;
            let right = b * c;
            // A rounded zero determinant is not evidence of an empty image.
            // FMA exposes product rounding; underflow remains a typed rejection.
            if (a != 0.0 && d != 0.0 && left.abs() < f64::MIN_POSITIVE)
                || (b != 0.0 && c != 0.0 && right.abs() < f64::MIN_POSITIVE)
                || a.mul_add(d, -left) != b.mul_add(c, -right)
            {
                return Err(GeometryError::NonFiniteInverse);
            }
            true
        } else {
            false
        };
        let inverse = if singular {
            None
        } else {
            let inverse = glam::DMat4::from_scale(glam::dvec3(1.0 / scale, 1.0 / scale, 1.0))
                * glam::DMat4::from_cols_array(&normalized).inverse();
            if !inverse.is_finite() {
                return Err(GeometryError::NonFiniteInverse);
            }
            Some(affine_snapshot(inverse))
        };
        Ok(Self {
            matrix: affine_snapshot(matrix),
            inverse,
        })
    }

    pub(crate) fn is_empty(self) -> bool {
        self.inverse.is_none()
    }

    pub(crate) fn matrix(self) -> glam::DMat4 {
        let coefficients = self.matrix.to_cols_array();
        glam::DMat4::from_cols(
            glam::dvec4(coefficients[0], coefficients[1], 0.0, 0.0),
            glam::dvec4(coefficients[2], coefficients[3], 0.0, 0.0),
            glam::dvec4(0.0, 0.0, 1.0, 0.0),
            glam::dvec4(coefficients[4], coefficients[5], 0.0, 1.0),
        )
    }

    pub(crate) fn map_point(self, point: Point<f64>) -> Result<Point<f64>, GeometryError> {
        validate_point(point)?;
        let mapped = self.matrix.transform_point2(glam::dvec2(point.x, point.y));
        let point = Point::new(mapped.x, mapped.y);
        validate_point(point)?;
        Ok(point)
    }

    pub(crate) fn map_bounds(self, rect: Rect<f64>) -> Result<Option<Rect<f64>>, GeometryError> {
        validate_rect(rect)?;
        if self.is_empty() {
            return Ok(None);
        }
        let corners = [
            self.map_point(Point::new(rect.left(), rect.top()))?,
            self.map_point(Point::new(rect.right(), rect.top()))?,
            self.map_point(Point::new(rect.right(), rect.bottom()))?,
            self.map_point(Point::new(rect.left(), rect.bottom()))?,
        ];
        let bounds = Rect::from_ltrb(
            corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            corners
                .iter()
                .map(|p| p.x)
                .fold(f64::NEG_INFINITY, f64::max),
            corners
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max),
        );
        validate_rect(bounds)?;
        Ok(Some(bounds))
    }

    pub(crate) fn inverse_coefficients(self) -> Result<Option<[f32; 6]>, GeometryError> {
        let Some(inverse) = self.inverse else {
            return Ok(None);
        };
        let coefficients = inverse.to_cols_array();
        let mut packed = [0.0; 6];
        for (output, value) in packed.iter_mut().zip(coefficients) {
            *output = pack_clip_value(value, "clip inverse")?;
        }
        Ok(Some(packed))
    }
}

fn affine_snapshot(matrix: glam::DMat4) -> glam::DAffine2 {
    let coefficients = matrix.to_cols_array();
    glam::DAffine2::from_mat2_translation(
        glam::DMat2::from_cols(
            glam::dvec2(coefficients[0], coefficients[1]),
            glam::dvec2(coefficients[4], coefficients[5]),
        ),
        glam::dvec2(coefficients[12], coefficients[13]),
    )
}

/// Validated owned shapes retain f64 radii, curves and the original fill rule.
#[derive(Clone, Debug)]
pub(crate) enum ValidatedClip {
    Rect(Rect<f64>),
    RRect(RRect),
    RSuperellipse(RSuperellipse),
    Path(Path),
}

impl ValidatedClip {
    pub(crate) fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            _ => None,
        }
    }
    pub(crate) fn rect(rect: Rect<f64>) -> Result<Self, GeometryError> {
        validate_rect(rect)?;
        Ok(Self::Rect(rect))
    }
    pub(crate) fn rrect(rect: RRect) -> Result<Self, GeometryError> {
        validate_rect(rect.rect)?;
        validate_radii([
            rect.top_left,
            rect.top_right,
            rect.bottom_right,
            rect.bottom_left,
        ])?;
        Ok(Self::RRect(rect))
    }
    pub(crate) fn rsuperellipse(rect: RSuperellipse) -> Result<Self, GeometryError> {
        validate_rect(rect.outer_rect())?;
        validate_radii([
            rect.tl_radius(),
            rect.tr_radius(),
            rect.br_radius(),
            rect.bl_radius(),
        ])?;
        Ok(Self::RSuperellipse(rect))
    }
    pub(crate) fn path(path: &Path, command_limit: usize) -> Result<Self, GeometryError> {
        let requested = path.commands().len();
        if requested > command_limit {
            return Err(GeometryError::PathCommandLimit {
                requested,
                limit: command_limit,
            });
        }
        for command in path.commands() {
            match command {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => validate_point(p)?,
                PathCommand::QuadraticTo(a, b) => {
                    validate_point(a)?;
                    validate_point(b)?;
                }
                PathCommand::CubicTo(a, b, c) => {
                    validate_point(a)?;
                    validate_point(b)?;
                    validate_point(c)?;
                }
                PathCommand::Close => {}
            }
        }
        // COW clone freezes the validated commands; this is not flattening.
        // Lowering must separately admit generated segments and scratch storage.
        Ok(Self::Path(path.clone()))
    }
}

fn validate_point(point: Point<f64>) -> Result<(), GeometryError> {
    if point.x.is_finite() && point.y.is_finite() {
        Ok(())
    } else {
        Err(GeometryError::NonFinite {
            context: "clip point",
        })
    }
}

fn validate_rect(rect: Rect<f64>) -> Result<(), GeometryError> {
    validate_point(Point::new(rect.left(), rect.top()))?;
    validate_point(Point::new(rect.right(), rect.bottom()))?;
    if !rect.width().is_finite()
        || !rect.height().is_finite()
        || rect.width() < 0.0
        || rect.height() < 0.0
    {
        return Err(GeometryError::InvalidExtent);
    }
    Ok(())
}

fn validate_radii(radii: [Radius<f64>; 4]) -> Result<(), GeometryError> {
    if radii
        .iter()
        .all(|r| r.x.is_finite() && r.y.is_finite() && r.x >= 0.0 && r.y >= 0.0)
    {
        Ok(())
    } else {
        Err(GeometryError::InvalidRadius)
    }
}
