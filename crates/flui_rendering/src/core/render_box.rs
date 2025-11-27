//! Box protocol render trait.
//!
//! This module provides the `RenderBox<A>` trait for implementing render objects
//! that use the standard 2D box layout protocol.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<A> trait
//! ├── layout() → Size
//! ├── paint()
//! └── hit_test() → bool
//! ```
//!
//! # Design
//!
//! `RenderBox` is generic over:
//! - `A`: Arity - compile-time child count (Leaf, Single, Optional, Variable)
//!
//! The trait uses context-based API where `LayoutContext` and `PaintContext`
//! provide access to children and tree operations.

use flui_interaction::HitTestResult;
use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::arity::{Arity, Leaf};
use super::contexts::{HitTestContext, LayoutContext, PaintContext};
use super::protocol::BoxProtocol;

// ============================================================================
// RENDER BOX TRAIT
// ============================================================================

/// Box protocol render trait.
///
/// Implement this trait for render objects that use the standard 2D box layout.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Optional, Variable)
///
/// # Example
///
/// ```rust,ignore
/// impl RenderBox<Leaf> for RenderColoredBox {
///     fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
///     where
///         T: LayoutTree,
///     {
///         ctx.constraints.constrain(Size::new(100.0, 100.0))
///     }
///
///     fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>)
///     where
///         T: PaintTree,
///     {
///         ctx.canvas().rect(Rect::from_size(self.size), &self.paint);
///     }
/// }
/// ```
pub trait RenderBox<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the size of this render object given constraints.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Layout context with constraints, children access, and tree
    ///
    /// # Returns
    ///
    /// The computed size that satisfies the constraints.
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, A, BoxProtocol>) -> Size
    where
        T: super::render_tree::LayoutTree;

    /// Paints this render object to a canvas.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Paint context with offset, children access, canvas, and tree
    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, A>)
    where
        T: super::render_tree::PaintTree;

    /// Performs hit testing for pointer events.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Hit test context with position, geometry, children access
    /// * `result` - Hit test result to add entries to
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit.
    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, A, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: super::render_tree::HitTestTree,
    {
        // Default: test children first, then self
        let hit_children = self.hit_test_children(ctx, result);
        if hit_children || self.hit_test_self(ctx.position, ctx.size()) {
            ctx.add_to_result(result);
            return true;
        }
        false
    }

    /// Tests if the position hits this render object (excluding children).
    ///
    /// Override for opaque hit testing (e.g., buttons, interactive areas).
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _position: Offset, _size: Size) -> bool {
        false
    }

    /// Tests if the position hits any children.
    ///
    /// Default iterates children and tests each. Override for custom
    /// traversal (e.g., z-order, clipping regions).
    fn hit_test_children<T>(
        &self,
        _ctx: &HitTestContext<'_, T, A, BoxProtocol>,
        _result: &mut HitTestResult,
    ) -> bool
    where
        T: super::render_tree::HitTestTree,
    {
        // Default: no children hit (override for non-leaf)
        false
    }

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    ///
    /// Default implementation provides automatic downcasting for all types.
    fn as_any(&self) -> &dyn std::any::Any
    where
        Self: Sized,
    {
        self
    }

    /// Downcasts to mutable concrete type for mutation.
    ///
    /// Default implementation provides automatic downcasting for all types.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any
    where
        Self: Sized,
    {
        self
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for ergonomic render box operations.
pub trait RenderBoxExt<A: Arity>: RenderBox<A> {
    /// Checks if position is within the given size bounds.
    fn contains(&self, position: Offset, size: Size) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }
}

impl<A: Arity, R: RenderBox<A>> RenderBoxExt<A> for R {}

// ============================================================================
// EMPTY RENDER
// ============================================================================

/// Empty render object with zero size.
///
/// Used for `Option::None` and placeholder elements.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyRender;

impl RenderBox<Leaf> for EmptyRender {
    fn layout<T>(&mut self, _ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size
    where
        T: super::render_tree::LayoutTree,
    {
        Size::ZERO
    }

    fn paint<T>(&self, _ctx: &mut PaintContext<'_, T, Leaf>)
    where
        T: super::render_tree::PaintTree,
    {
        // Nothing to paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_render_default() {
        let empty = EmptyRender::default();
        assert_eq!(
            empty.debug_name(),
            "flui_rendering::core::render_box::EmptyRender"
        );
    }
}
