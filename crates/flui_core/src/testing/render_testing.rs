//! Testing helpers for RenderObjects
//!
//! Provides utilities for testing RenderObjects in isolation, including:
//! - Mock RenderContext for tests
//! - Layout testing helpers
//! - Assertions for render objects

use crate::{BoxConstraints, ElementTree, RenderContext, DynRenderObject};
use flui_types::Size;

/// Create a mock RenderContext for testing
///
/// Creates an empty ElementTree and RenderContext that can be used
/// in RenderObject tests where children aren't needed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::mock_render_context;
///
/// #[test]
/// fn test_layout() {
///     let (tree, ctx) = mock_render_context();
///     let mut render_obj = RenderPadding::new(...);
///
///     let size = render_obj.layout(constraints, &ctx);
///     assert_eq!(size.width, 100.0);
/// }
/// ```
pub fn mock_render_context() -> (ElementTree, RenderContext<'static>) {
    // Create a static empty tree for testing
    // SAFETY: This is safe for tests because the tree lives for the test duration
    let tree = Box::leak(Box::new(ElementTree::new()));
    let ctx = RenderContext::new(tree, 0);

    // Return both so the tree can be dropped if needed
    // In practice, tests just use the ctx and let it leak (it's just a test)
    (unsafe { std::ptr::read(tree) }, ctx)
}

/// Helper to test layout of a RenderObject
///
/// Calls layout() on the render object and returns the size.
/// Uses a mock context if the render object doesn't need children.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::test_layout;
///
/// #[test]
/// fn test_padding_layout() {
///     let mut padding = RenderPadding::new(...);
///     let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
///
///     let size = test_layout(&padding, constraints);
///     assert_eq!(size, Size::new(100.0, 100.0));
/// }
/// ```
pub fn test_layout(render_object: &dyn DynRenderObject, constraints: BoxConstraints) -> Size {
    let (_tree, ctx) = mock_render_context();
    let mut state = crate::RenderState::new();
    render_object.layout(&mut state, constraints, &ctx)
}

/// Assert that layout returns expected size
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::assert_layout_size;
///
/// #[test]
/// fn test_constrained_box() {
///     let render_obj = RenderConstrainedBox::new(...);
///     let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
///
///     assert_layout_size(&render_obj, constraints, Size::new(100.0, 50.0));
/// }
/// ```
pub fn assert_layout_size(
    render_object: &dyn DynRenderObject,
    constraints: BoxConstraints,
    expected: Size,
) {
    let size = test_layout(render_object, constraints);
    assert_eq!(
        size, expected,
        "Layout size mismatch: expected {:?}, got {:?}",
        expected, size
    );
}

/// Assert that layout returns a size within constraints
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::assert_layout_within_constraints;
///
/// #[test]
/// fn test_flex_layout() {
///     let flex = RenderFlex::new(...);
///     let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
///
///     assert_layout_within_constraints(&flex, constraints);
/// }
/// ```
pub fn assert_layout_within_constraints(
    render_object: &dyn DynRenderObject,
    constraints: BoxConstraints,
) {
    let size = test_layout(render_object, constraints);
    assert!(
        constraints.is_satisfied_by(size),
        "Layout size {:?} does not satisfy constraints {:?}",
        size,
        constraints
    );
}

/// Create tight constraints for testing
///
/// Shorthand for creating BoxConstraints with min == max.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::tight;
///
/// let constraints = tight(100.0, 50.0);
/// assert!(constraints.is_tight());
/// ```
#[inline]
pub fn tight(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::tight(Size::new(width, height))
}

/// Create loose constraints for testing
///
/// Shorthand for creating BoxConstraints with min = 0.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::loose;
///
/// let constraints = loose(100.0, 50.0);
/// assert!(!constraints.is_tight());
/// ```
#[inline]
pub fn loose(max_width: f32, max_height: f32) -> BoxConstraints {
    BoxConstraints::new(0.0, max_width, 0.0, max_height)
}

/// Create unbounded constraints for testing
///
/// Useful for testing intrinsic sizing.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::unbounded;
///
/// let constraints = unbounded();
/// assert_eq!(constraints.max_width, f32::INFINITY);
/// ```
#[inline]
pub fn unbounded() -> BoxConstraints {
    BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_render_context_creation() {
        let (_tree, ctx) = mock_render_context();

        // Context should be valid
        assert_eq!(ctx.element_id(), 0);
        assert_eq!(ctx.children().len(), 0);
    }

    #[test]
    fn test_tight_constraints() {
        let constraints = tight(100.0, 50.0);

        assert!(constraints.is_tight());
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_loose_constraints() {
        let constraints = loose(100.0, 50.0);

        assert!(!constraints.is_tight());
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_unbounded_constraints() {
        let constraints = unbounded();

        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, f32::INFINITY);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, f32::INFINITY);
    }
}
