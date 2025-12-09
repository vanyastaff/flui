//! Debug utilities and assertions for layout protocol validation.
//!
//! This module provides compile-time and runtime checks to ensure
//! correct usage of the Flutter-style layout protocol.
//!
//! # Features
//!
//! - **Constraint Validation**: Ensures constraints are normalized and valid
//! - **Size Validation**: Ensures returned sizes satisfy constraints
//! - **Layout Order Validation**: Detects incorrect layout ordering
//! - **Memory Layout Verification**: Ensures protocol types have correct layout
//!
//! # Usage
//!
//! All assertions are only active in debug builds (`#[cfg(debug_assertions)]`).
//! In release builds, they compile to no-ops for zero overhead.

use flui_types::{BoxConstraints, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// CONSTRAINT VALIDATION
// ============================================================================

/// Validates that BoxConstraints are normalized (min <= max).
///
/// # Panics (Debug Only)
///
/// Panics if constraints are not normalized:
/// - `min_width > max_width`
/// - `min_height > max_height`
///
/// # Example
///
/// ```rust,ignore
/// debug_assert_constraints_normalized(&constraints);
/// ```
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_constraints_normalized(constraints: &BoxConstraints) {
    assert!(
        constraints.min_width <= constraints.max_width,
        "BoxConstraints min_width ({}) > max_width ({}). \
         Constraints must be normalized (min <= max).",
        constraints.min_width,
        constraints.max_width
    );
    assert!(
        constraints.min_height <= constraints.max_height,
        "BoxConstraints min_height ({}) > max_height ({}). \
         Constraints must be normalized (min <= max).",
        constraints.min_height,
        constraints.max_height
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_constraints_normalized(_constraints: &BoxConstraints) {}

/// Validates that BoxConstraints don't contain NaN or negative infinity.
///
/// # Panics (Debug Only)
///
/// Panics if any value is NaN or negative infinity.
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_constraints_finite(constraints: &BoxConstraints) {
    assert!(
        !constraints.min_width.is_nan() && constraints.min_width >= 0.0,
        "BoxConstraints min_width is invalid: {}",
        constraints.min_width
    );
    assert!(
        !constraints.max_width.is_nan(),
        "BoxConstraints max_width is NaN"
    );
    assert!(
        !constraints.min_height.is_nan() && constraints.min_height >= 0.0,
        "BoxConstraints min_height is invalid: {}",
        constraints.min_height
    );
    assert!(
        !constraints.max_height.is_nan(),
        "BoxConstraints max_height is NaN"
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_constraints_finite(_constraints: &BoxConstraints) {}

/// Validates BoxConstraints are valid (normalized and finite).
#[inline]
pub fn debug_assert_valid_constraints(constraints: &BoxConstraints) {
    debug_assert_constraints_finite(constraints);
    debug_assert_constraints_normalized(constraints);
}

// ============================================================================
// SIZE VALIDATION
// ============================================================================

/// Validates that a Size satisfies the given BoxConstraints.
///
/// This is the core Flutter layout protocol check. Every layout() method
/// MUST return a size that satisfies its input constraints.
///
/// # Panics (Debug Only)
///
/// Panics if size violates constraints:
/// - `size.width < constraints.min_width`
/// - `size.width > constraints.max_width`
/// - `size.height < constraints.min_height`
/// - `size.height > constraints.max_height`
///
/// # Example
///
/// ```rust,ignore
/// fn layout(&mut self, ctx: BoxLayoutContext) -> RenderResult<Size> {
///     let size = compute_size();
///     debug_assert_size_satisfies_constraints(size, &ctx.constraints);
///     Ok(size)
/// }
/// ```
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_size_satisfies_constraints(size: Size, constraints: &BoxConstraints) {
    // Allow small floating point errors (epsilon)
    const EPSILON: f32 = 0.001;

    let satisfies_width = size.width >= constraints.min_width - EPSILON
        && (constraints.max_width.is_infinite() || size.width <= constraints.max_width + EPSILON);

    let satisfies_height = size.height >= constraints.min_height - EPSILON
        && (constraints.max_height.is_infinite()
            || size.height <= constraints.max_height + EPSILON);

    assert!(
        satisfies_width,
        "Layout returned width {} which violates constraints [{}, {}]. \
         This is a Flutter layout protocol violation. \
         The size returned by layout() MUST satisfy the input constraints.",
        size.width, constraints.min_width, constraints.max_width
    );

    assert!(
        satisfies_height,
        "Layout returned height {} which violates constraints [{}, {}]. \
         This is a Flutter layout protocol violation. \
         The size returned by layout() MUST satisfy the input constraints.",
        size.height, constraints.min_height, constraints.max_height
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_size_satisfies_constraints(_size: Size, _constraints: &BoxConstraints) {}

/// Validates that a Size contains no NaN values.
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_size_finite(size: Size) {
    assert!(
        !size.width.is_nan() && size.width >= 0.0,
        "Size width is invalid: {}. Size must be non-negative and finite.",
        size.width
    );
    assert!(
        !size.height.is_nan() && size.height >= 0.0,
        "Size height is invalid: {}. Size must be non-negative and finite.",
        size.height
    );
    // Note: Infinite size is allowed in some cases (e.g., intrinsic measurement)
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_size_finite(_size: Size) {}

// ============================================================================
// SLIVER CONSTRAINT VALIDATION
// ============================================================================

/// Validates SliverConstraints are valid.
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_valid_sliver_constraints(constraints: &SliverConstraints) {
    assert!(
        !constraints.scroll_offset.is_nan() && constraints.scroll_offset >= 0.0,
        "SliverConstraints scroll_offset is invalid: {}",
        constraints.scroll_offset
    );
    assert!(
        !constraints.remaining_paint_extent.is_nan() && constraints.remaining_paint_extent >= 0.0,
        "SliverConstraints remaining_paint_extent is invalid: {}",
        constraints.remaining_paint_extent
    );
    assert!(
        !constraints.viewport_main_axis_extent.is_nan()
            && constraints.viewport_main_axis_extent >= 0.0,
        "SliverConstraints viewport_main_axis_extent is invalid: {}",
        constraints.viewport_main_axis_extent
    );
    assert!(
        !constraints.cross_axis_extent.is_nan() && constraints.cross_axis_extent >= 0.0,
        "SliverConstraints cross_axis_extent is invalid: {}",
        constraints.cross_axis_extent
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_valid_sliver_constraints(_constraints: &SliverConstraints) {}

/// Validates SliverGeometry is valid.
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_valid_sliver_geometry(geometry: &SliverGeometry) {
    assert!(
        !geometry.scroll_extent.is_nan() && geometry.scroll_extent >= 0.0,
        "SliverGeometry scroll_extent is invalid: {}",
        geometry.scroll_extent
    );
    assert!(
        !geometry.paint_extent.is_nan() && geometry.paint_extent >= 0.0,
        "SliverGeometry paint_extent is invalid: {}",
        geometry.paint_extent
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_valid_sliver_geometry(_geometry: &SliverGeometry) {}

// ============================================================================
// LAYOUT STATE ASSERTIONS
// ============================================================================

/// Debug context for tracking layout state.
///
/// This helps detect incorrect layout ordering such as:
/// - Querying child size before layout
/// - Calling layout during paint
/// - Recursive layout without proper dirty tracking
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPhase {
    /// Not in any phase
    #[default]
    Idle,
    /// Currently performing layout
    Layout,
    /// Currently performing paint
    Paint,
    /// Currently performing hit test
    HitTest,
}

/// Asserts that we are in layout phase (for operations that require it).
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_in_layout_phase(phase: LayoutPhase) {
    assert_eq!(
        phase,
        LayoutPhase::Layout,
        "This operation is only valid during the layout phase. \
         Current phase: {:?}. \
         Did you try to access child size during paint?",
        phase
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_in_layout_phase(_phase: ()) {}

/// Asserts that we are NOT in paint phase (layout operations are forbidden during paint).
#[cfg(debug_assertions)]
#[inline]
pub fn debug_assert_not_in_paint_phase(phase: LayoutPhase) {
    assert_ne!(
        phase,
        LayoutPhase::Paint,
        "Layout operations are forbidden during the paint phase. \
         This is a Flutter layout protocol violation. \
         The paint() method must not trigger layout."
    );
}

#[cfg(not(debug_assertions))]
#[inline]
pub fn debug_assert_not_in_paint_phase(_phase: ()) {}

// ============================================================================
// PROTOCOL TYPE ID VERIFICATION
// ============================================================================

/// Verifies that our TypeId-based protocol casting is sound.
///
/// # Background
///
/// In `RenderElement::as_box_state()`, we use TypeId to check if `P == BoxProtocol`
/// before casting. When the TypeId check passes, we know `P` IS `BoxProtocol`,
/// so we're casting `&RenderState<BoxProtocol>` to `&RenderState<BoxProtocol>`.
/// This is a same-type cast which is always safe.
///
/// This function verifies:
/// 1. TypeId correctly distinguishes between protocol types
/// 2. Same protocol types have identical TypeIds
///
/// # Note
///
/// We do NOT require `BoxRenderState` and `SliverRenderState` to have the same
/// memory layout because we never cast between them. The TypeId check ensures
/// we only cast when `P == BoxProtocol` (or `P == SliverProtocol`), meaning
/// we cast `&RenderState<P>` to `&RenderState<P>` - a same-type identity cast.
#[cfg(debug_assertions)]
pub fn verify_protocol_type_safety() {
    use crate::core::protocol::{BoxProtocol, SliverProtocol};
    use std::any::TypeId;

    // TypeId should be equal for the same type
    let box_id_1 = TypeId::of::<BoxProtocol>();
    let box_id_2 = TypeId::of::<BoxProtocol>();
    assert_eq!(
        box_id_1, box_id_2,
        "TypeId::of::<BoxProtocol>() is not stable!"
    );

    let sliver_id_1 = TypeId::of::<SliverProtocol>();
    let sliver_id_2 = TypeId::of::<SliverProtocol>();
    assert_eq!(
        sliver_id_1, sliver_id_2,
        "TypeId::of::<SliverProtocol>() is not stable!"
    );

    // Different types should have different TypeIds
    assert_ne!(
        box_id_1, sliver_id_1,
        "BoxProtocol and SliverProtocol have the same TypeId! \
         This would break our protocol casting logic."
    );
}

#[cfg(not(debug_assertions))]
pub fn verify_protocol_type_safety() {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_constraints() {
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        debug_assert_valid_constraints(&constraints);
    }

    #[test]
    #[should_panic(expected = "min_width")]
    #[cfg(debug_assertions)]
    fn test_invalid_constraints_min_greater_than_max() {
        let constraints = BoxConstraints::new(100.0, 50.0, 0.0, 100.0);
        debug_assert_constraints_normalized(&constraints);
    }

    #[test]
    fn test_size_satisfies_constraints() {
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let size = Size::new(50.0, 50.0);
        debug_assert_size_satisfies_constraints(size, &constraints);
    }

    #[test]
    #[should_panic(expected = "violates constraints")]
    #[cfg(debug_assertions)]
    fn test_size_violates_constraints() {
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let size = Size::new(150.0, 50.0); // Width exceeds max
        debug_assert_size_satisfies_constraints(size, &constraints);
    }

    #[test]
    fn test_protocol_type_safety() {
        verify_protocol_type_safety();
    }

    #[test]
    fn test_size_finite() {
        let size = Size::new(100.0, 50.0);
        debug_assert_size_finite(size);
    }

    #[test]
    #[should_panic(expected = "invalid")]
    #[cfg(debug_assertions)]
    fn test_size_nan_panics() {
        let size = Size::new(f32::NAN, 50.0);
        debug_assert_size_finite(size);
    }
}
