//! Matrix math of a gesture: composing a transform in viewport space and
//! clamping its translation and scale to the boundary and the scale bounds.

use flui_foundation::geometry::{Axis, Matrix4, Offset, Rect};

/// Compose in viewport space, then keep the same scene point under the moving
/// focal point. Publish only an invertible finite matrix inside the boundary.
#[expect(
    clippy::too_many_arguments,
    reason = "the gesture state is a set of independent scalars and offsets that no existing struct groups"
)]
pub(super) fn gesture_transform(
    matrix: Matrix4,
    focal: Offset<f64>,
    delta: Offset<f64>,
    scale: f64,
    rotation: f64,
    min_scale: f64,
    max_scale: f64,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Option<Matrix4> {
    if !focal.is_finite()
        || !delta.is_finite()
        || scale.is_nan()
        || scale < 0.0
        || !rotation.is_finite()
        || !viewport.is_finite()
        || viewport.width() <= 0.0
        || viewport.height() <= 0.0
    {
        return None;
    }
    if delta == Offset::ZERO && scale == 1.0 && rotation == 0.0 {
        return contain_transform(matrix, viewport, boundary);
    }
    let previous_focal = focal - delta;
    if !previous_focal.is_finite() {
        return None;
    }
    let inverse = matrix.try_inverse()?;
    let pivot = inverse.transform_point(previous_focal.dx, previous_focal.dy);
    if !pivot.0.is_finite() || !pivot.1.is_finite() {
        return None;
    }
    let current_scale = uniform_scale(&matrix);
    if !current_scale.is_finite() || current_scale <= 0.0 {
        return None;
    }
    let scaled = clamp_scale(matrix, scale, min_scale, max_scale, viewport, boundary);
    let mut next = Matrix4::rotation_z(rotation) * scaled;
    let mapped = next.transform_point(pivot.0, pivot.1);
    let correction = focal - Offset::new(mapped.0, mapped.1);
    if !correction.is_finite() {
        return None;
    }
    next = Matrix4::translation(correction.dx, correction.dy, 0.0) * next;
    contain_transform(next, viewport, boundary)
}

pub(super) fn contain_transform(
    mut matrix: Matrix4,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Option<Matrix4> {
    if !matrix
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let values = matrix.to_col_major_array();
    if boundary.is_finite()
        && values[3] == 0.0
        && values[7] == 0.0
        && values[11] == 0.0
        && values[15] == 1.0
    {
        // Separate translation from the viewport's shape. Subtracting a huge
        // excess from a huge translation loses the small boundary coordinate.
        let (x, y, z) = matrix.translation_component();
        let mut linear = matrix;
        linear.set_translation(0.0, 0.0, 0.0);
        let inverse = linear.try_inverse()?;
        let shape = inverse.transform_rect(&viewport);
        if !shape.is_finite()
            || shape.width() > boundary.width() + EXCESS_EPSILON
            || shape.height() > boundary.height() + EXCESS_EPSILON
        {
            return None;
        }
        let shift = inverse.transform_point(-x, -y);
        if !shift.0.is_finite() || !shift.1.is_finite() {
            return None;
        }
        let lower = Offset::new(boundary.min.x - shape.min.x, boundary.min.y - shape.min.y);
        let upper = Offset::new(boundary.max.x - shape.max.x, boundary.max.y - shape.max.y);
        if !lower.is_finite() || !upper.is_finite() {
            return None;
        }
        let clamped = Offset::new(
            shift.0.clamp(lower.dx, upper.dx.max(lower.dx)),
            shift.1.clamp(lower.dy, upper.dy.max(lower.dy)),
        );
        if clamped.dx != shift.0 || clamped.dy != shift.1 {
            let translated = linear.transform_point(-clamped.dx, -clamped.dy);
            if !translated.0.is_finite() || !translated.1.is_finite() {
                return None;
            }
            matrix.set_translation(translated.0, translated.1, z);
        }
    }
    let inverse = matrix.try_inverse()?;
    let scene_viewport = inverse.transform_rect(&viewport);
    if !scene_viewport.is_finite() {
        return None;
    }
    if !boundary.is_finite() {
        return (![
            boundary.min.x,
            boundary.min.y,
            boundary.max.x,
            boundary.max.y,
        ]
        .iter()
        .any(|value| value.is_nan()))
        .then_some(matrix);
    }
    // A rotated viewport is a quad; its bounding rectangle fits this
    // axis-aligned scene boundary exactly when all four corners fit. If its
    // extent cannot fit at the admitted scale, reject rather than invent zoom.
    if scene_viewport.width() > boundary.width() + EXCESS_EPSILON
        || scene_viewport.height() > boundary.height() + EXCESS_EPSILON
    {
        return None;
    }
    let excess = rect_excess(boundary, scene_viewport);
    if !excess.is_finite() {
        return None;
    }
    if !excess_is_negligible(excess) {
        matrix *= Matrix4::translation(-excess.dx, -excess.dy, 0.0);
    }
    let result = matrix.try_inverse()?.transform_rect(&viewport);
    (matrix
        .to_col_major_array()
        .iter()
        .all(|value| value.is_finite())
        && result.is_finite()
        && excess_is_negligible(rect_excess(boundary, result)))
    .then_some(matrix)
}

/// Uniform scale of a translated/rotated matrix: length of its x basis vector.
pub(super) fn uniform_scale(matrix: &Matrix4) -> f64 {
    let m = matrix.to_col_major_array();
    m[0].hypot(m[1])
}

/// Transforms `viewport`'s four corners by the inverse of `matrix` and
/// returns their axis-aligned bounding box — the viewport's rect in scene
/// coordinates after the child has been transformed by `matrix`. Falls back
/// to `viewport` unchanged if `matrix` is singular (should not happen for a
/// translation + uniform-scale matrix with a non-zero scale).
pub(super) fn transform_viewport(matrix: Matrix4, viewport: Rect<f64>) -> Rect<f64> {
    match matrix.try_inverse() {
        Some(inverse) => inverse.transform_rect(&viewport),
        None => viewport,
    }
}

/// How far `[view_min, view_max]` lies outside `[bound_min, bound_max]` along
/// one axis, signed so that adding it to the viewport's position moves it
/// back inside the boundary. Zero when already inside (inclusive).
///
/// The caller supplies the transformed viewport's bounding rectangle, so the
/// interval comparison also covers rotation. If the viewport is wider than
/// the boundary, the edge with the larger-magnitude excess wins.
pub(super) fn axis_excess(view_min: f64, view_max: f64, bound_min: f64, bound_max: f64) -> f64 {
    let excess_min = if view_min < bound_min {
        bound_min - view_min
    } else {
        0.0
    };
    let excess_max = if view_max > bound_max {
        bound_max - view_max
    } else {
        0.0
    };
    if excess_min.abs() >= excess_max.abs() {
        excess_min
    } else {
        excess_max
    }
}

pub(super) fn rect_excess(boundary: Rect<f64>, viewport: Rect<f64>) -> Offset<f64> {
    Offset::new(
        axis_excess(
            viewport.min.x,
            viewport.max.x,
            boundary.min.x,
            boundary.max.x,
        ),
        axis_excess(
            viewport.min.y,
            viewport.max.y,
            boundary.min.y,
            boundary.max.y,
        ),
    )
}

/// Floating-point tolerance for the "did this transform round-trip produce
/// zero excess" checks in [`clamp_translation`].
///
/// The viewport's inverse-then-transform round trip leaves residue that
/// *should* be exactly zero but isn't once the matrix carries a non-unit (and
/// non-power-of-two) scale. This snaps anything within `EXCESS_EPSILON` of
/// zero back to exactly zero. Chosen against the scale of one gesture's excess (tens to
/// thousands of pixels) rather than absolute machine epsilon — comfortably
/// larger than the ~1e-4 residue a `scale * (a - b)` round trip leaves at
/// these magnitudes, comfortably smaller than any excess a real boundary
/// hit produces.
pub(super) const EXCESS_EPSILON: f64 = 1e-3;

/// Whether a single excess component is within [`EXCESS_EPSILON`] of zero.
pub(super) fn is_negligible(component: f64) -> bool {
    component.abs() < EXCESS_EPSILON
}

/// Whether `excess` is within [`EXCESS_EPSILON`] of `Offset::ZERO` on both
/// axes — the round-trip-tolerant replacement for `excess == Offset::ZERO`.
pub(super) fn excess_is_negligible(excess: Offset<f64>) -> bool {
    is_negligible(excess.dx) && is_negligible(excess.dy)
}

/// Locks a `PanAxis::Aligned` drag to whichever axis dominates `delta`.
/// `delta` must be non-zero (callers only invoke this on real movement).
pub(super) fn dominant_axis(delta: Offset<f64>) -> Axis {
    if delta.dx.abs() > delta.dy.abs() {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

/// Zeroes out the off-axis component of `delta`.
pub(super) fn align_to_axis(delta: Offset<f64>, axis: Axis) -> Offset<f64> {
    match axis {
        Axis::Horizontal => Offset::new(delta.dx, 0.0),
        Axis::Vertical => Offset::new(0.0, delta.dy),
    }
}

/// Applies `translation` (in scene units) to `matrix`, clamped so the
/// transformed viewport stays within `boundary` when `boundary` is finite.
///
/// Composed via
/// `matrix * Matrix4::translation(..)` (post-multiply — the translation
/// happens in the matrix's own local/scene space before the rest of the
/// transform is applied). `flui_foundation::geometry`'s
/// own `Matrix4::translate` mutator has the *opposite* (pre-multiply,
/// global-space) convention and must not be used here.
pub(super) fn clamp_translation(
    matrix: Matrix4,
    translation: Offset<f64>,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Matrix4 {
    if translation == Offset::ZERO {
        return matrix;
    }
    let next = matrix * Matrix4::translation(translation.dx, translation.dy, 0.0);

    if !boundary.is_finite() {
        return next;
    }

    let next_viewport = transform_viewport(next, viewport);
    let excess = rect_excess(boundary, next_viewport);
    if excess_is_negligible(excess) {
        return next;
    }

    let (next_tx, next_ty, next_tz) = next.translation_component();
    let current_scale = uniform_scale(&matrix);
    let corrected_tx = next_tx - excess.dx * current_scale;
    let corrected_ty = next_ty - excess.dy * current_scale;
    let mut corrected = matrix;
    corrected.set_translation(corrected_tx, corrected_ty, next_tz);

    let corrected_viewport = transform_viewport(corrected, viewport);
    let corrected_excess = rect_excess(boundary, corrected_viewport);
    if excess_is_negligible(corrected_excess) {
        return corrected;
    }

    if !is_negligible(corrected_excess.dx) && !is_negligible(corrected_excess.dy) {
        // Neither axis fits at all (the viewport is larger than the
        // boundary in both directions): no translation.
        return matrix;
    }

    let unidirectional_tx = if is_negligible(corrected_excess.dx) {
        corrected_tx
    } else {
        0.0
    };
    let unidirectional_ty = if is_negligible(corrected_excess.dy) {
        corrected_ty
    } else {
        0.0
    };
    let mut result = matrix;
    result.set_translation(unidirectional_tx, unidirectional_ty, next_tz);
    result
}

/// Applies `scale` (a multiplicative change) to `matrix`, clamped so the
/// resulting overall scale stays within `[min_scale, max_scale]` and never
/// shrinks the child so much it can't cover `boundary` from `viewport`.
///
/// Composed via
/// `matrix * Matrix4::scaling(..)` — see [`clamp_translation`]'s doc for why
/// the mutating `Matrix4::scale` method is the wrong tool here.
pub(super) fn clamp_scale(
    matrix: Matrix4,
    scale: f64,
    min_scale: f64,
    max_scale: f64,
    viewport: Rect<f64>,
    boundary: Rect<f64>,
) -> Matrix4 {
    if scale == 1.0 {
        return matrix;
    }
    // Debug-only: this does not replace `clamp_double`'s non-panicking
    // behavior below; it only surfaces the misconfiguration in debug builds.
    debug_assert!(
        max_scale >= min_scale,
        "InteractiveViewer: max_scale ({max_scale}) must be >= min_scale ({min_scale})"
    );
    let current_scale = uniform_scale(&matrix);
    // Finite / infinite (unbounded boundary) is naturally 0.0 here — no
    // separate infinite-boundary branch needed.
    let boundary_floor =
        (viewport.width() / boundary.width()).max(viewport.height() / boundary.height());
    let total_scale = (current_scale * scale).max(boundary_floor);
    let clamped_total = clamp_double(total_scale, min_scale, max_scale);
    let applied = clamped_total / current_scale;
    // The plane alone: depth keeps its unit scale, so the transform stays the
    // two-dimensional affine one the compositor draws (ADR-0102).
    matrix * Matrix4::scaling(applied, applied, 1.0)
}

/// Unlike `f64::clamp`
/// (which panics — in every build profile, not just debug — whenever `min >
/// max`), this never panics: a misconfigured `min_scale > max_scale` falls
/// through (the assertion above is debug-only) instead of crashing a release build over a caller error that
/// should have been caught in testing.
pub(super) fn clamp_double(x: f64, min: f64, max: f64) -> f64 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}
