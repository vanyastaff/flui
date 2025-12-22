//! Rendering phase system for type-safe API enforcement.
//!
//! This module defines zero-sized phase markers that are used throughout
//! the rendering system to enforce correct API usage at compile time.
//!
//! Different rendering phases have different capabilities:
//!
//! - **Layout Phase**: Can layout children, modify ParentData, compute intrinsics
//! - **Paint Phase**: Can only paint children (read-only access)
//! - **Hit Test Phase**: Can only perform hit testing (read-only access)
//!
//! By parameterizing types like `ChildHandle<P, Phase>` and `Context<A, P, Phase>`
//! with these phase markers, we ensure that operations are only available in
//! the correct phase. For example, you cannot call `child.paint()` during layout,
//! or `child.layout()` during paint - these will be compile errors.

use std::fmt::Debug;

// ============================================================================
// Phase Markers (Zero-Sized Types)
// ============================================================================

/// Marker type for the layout phase.
///
/// During layout, a render object:
/// - Receives constraints from its parent
/// - Lays out its children with appropriate constraints
/// - Positions children by setting offsets in ParentData
/// - Computes and returns its own size
///
/// # Available Operations
///
/// - Layout children: `child.layout(constraints)`
/// - Set positions: `child.set_offset(offset)`
/// - Modify ParentData: `child.parent_data_mut()`
/// - Compute intrinsics: `child.get_min_intrinsic_width()`
/// - Dry layout: `child.dry_layout(constraints)`
/// - Baseline queries: `child.get_distance_to_baseline()`
///
/// # Unavailable Operations
///
/// - ❌ Painting: `child.paint()` - compile error
/// - ❌ Hit testing: `child.hit_test()` - compile error
///
/// # Example
///
/// ```ignore
/// // Layout context uses LayoutPhase
/// fn perform_layout(
///     &mut self,
///     ctx: BoxLayoutContext<'_, Optional, BoxParentData>  // LayoutPhase implicit
/// ) -> Size {
///     if let Some(mut child) = ctx.children.get() {
///         // child: ChildHandle<BoxParentData, LayoutPhase>
///         let size = child.layout(constraints);  // ✅ OK
///         child.set_offset(offset);              // ✅ OK
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutPhase;

/// Marker type for the paint phase.
///
/// During paint, a render object:
/// - Paints itself to the canvas
/// - Paints children at their offsets (from ParentData)
/// - May apply effects (opacity, clipping, transforms)
///
/// # Available Operations
///
/// - Paint children: `child.paint(context)`
/// - Paint at custom offset: `child.paint_at(context, offset)`
/// - Read ParentData: `child.parent_data()`
/// - Read offset: `child.offset()`
/// - Read size: `child.size()`
///
/// # Unavailable Operations
///
/// - ❌ Layout: `child.layout()` - compile error
/// - ❌ Modify ParentData: `child.set_offset()` - compile error
/// - ❌ Hit testing: `child.hit_test()` - compile error
///
/// # Example
///
/// ```ignore
/// // Paint context uses PaintPhase
/// fn paint(&self, ctx: BoxPaintContext<'_, Optional, BoxParentData>) {
///     if let Some(child) = ctx.children.get() {
///         // child: ChildHandle<BoxParentData, PaintPhase>
///         child.paint(&mut ctx.painting_context);  // ✅ OK
///
///         // child.layout(constraints);  // ❌ Compile error!
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaintPhase;

/// Marker type for the hit test phase.
///
/// During hit testing, a render object:
/// - Determines if a point intersects with itself or its children
/// - Adds itself to the hit test result if hit
/// - Delegates to children for hit testing
///
/// # Available Operations
///
/// - Hit test children: `child.hit_test(position)`
/// - Read ParentData: `child.parent_data()`
/// - Read offset: `child.offset()`
/// - Read size: `child.size()`
/// - Read bounds: `child.paint_bounds()`
///
/// # Unavailable Operations
///
/// - ❌ Layout: `child.layout()` - compile error
/// - ❌ Paint: `child.paint()` - compile error
/// - ❌ Modify ParentData: `child.set_offset()` - compile error
///
/// # Example
///
/// ```ignore
/// // Hit test context uses HitTestPhase
/// fn hit_test(
///     &self,
///     ctx: BoxHitTestContext<'_, Optional, BoxParentData>
/// ) -> bool {
///     if let Some(child) = ctx.children.get() {
///         // child: ChildHandle<BoxParentData, HitTestPhase>
///         if child.hit_test(position) {  // ✅ OK
///             return true;
///         }
///     }
///     false
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HitTestPhase;

// ============================================================================
// Phase Trait
// ============================================================================

/// Marker trait for rendering phases.
///
/// This trait is sealed and can only be implemented by the three phase markers
/// in this module. It provides a way to write generic code over phases while
/// maintaining type safety.
///
/// # Sealed Trait
///
/// This trait is sealed and cannot be implemented outside this crate.
/// Only `LayoutPhase`, `PaintPhase`, and `HitTestPhase` implement it.
pub trait Phase: private::Sealed + Debug + Clone + Copy + PartialEq + Eq + 'static {
    /// Human-readable name of the phase (for debugging).
    fn name() -> &'static str;
}

impl Phase for LayoutPhase {
    fn name() -> &'static str {
        "Layout"
    }
}

impl Phase for PaintPhase {
    fn name() -> &'static str {
        "Paint"
    }
}

impl Phase for HitTestPhase {
    fn name() -> &'static str {
        "HitTest"
    }
}

// ============================================================================
// Sealed Pattern (prevents external implementations)
// ============================================================================

mod private {
    use super::*;

    pub trait Sealed {}

    impl Sealed for LayoutPhase {}
    impl Sealed for PaintPhase {}
    impl Sealed for HitTestPhase {}
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_markers_are_zero_sized() {
        use std::mem::size_of;

        assert_eq!(size_of::<LayoutPhase>(), 0);
        assert_eq!(size_of::<PaintPhase>(), 0);
        assert_eq!(size_of::<HitTestPhase>(), 0);
    }

    #[test]
    fn test_phase_names() {
        assert_eq!(LayoutPhase::name(), "Layout");
        assert_eq!(PaintPhase::name(), "Paint");
        assert_eq!(HitTestPhase::name(), "HitTest");
    }

    #[test]
    fn test_phase_equality() {
        assert_eq!(LayoutPhase, LayoutPhase);
        assert_eq!(PaintPhase, PaintPhase);
        assert_eq!(HitTestPhase, HitTestPhase);

        assert_ne!(LayoutPhase, PaintPhase);
        assert_ne!(PaintPhase, HitTestPhase);
    }

    #[test]
    fn test_phase_copy_clone() {
        let layout = LayoutPhase;
        let layout_copy = layout;
        let layout_clone = layout.clone();

        assert_eq!(layout, layout_copy);
        assert_eq!(layout, layout_clone);
    }
}
