//! RenderView trait - Views that create render objects
//!
//! For views that participate in layout and painting through render objects.

use std::fmt::Debug;

use flui_rendering::core::{
    arity::Arity,
    protocol::{BoxProtocol, Protocol, SliverProtocol},
    RenderBox, RenderObject, RenderSliver,
};

use super::UpdateResult;

/// RenderView - Widget that creates render objects.
///
/// Similar to Flutter's `RenderObjectWidget`. This is a **widget** that
/// stores configuration and creates render objects.
///
/// # Type Parameters
///
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
/// - `A`: Arity (Leaf, Single, Optional, Variable)
///
/// # Example
///
/// ```rust,ignore
/// struct Padding {
///     padding: EdgeInsets,
///     child: Child,
/// }
///
/// impl RenderView<BoxProtocol, Single> for Padding {
///     type RenderObject = RenderPadding;
///
///     fn create(&self) -> RenderPadding {
///         RenderPadding::new(self.padding)
///     }
///
///     fn update(&self, render: &mut RenderPadding) -> UpdateResult {
///         if render.padding == self.padding {
///             return UpdateResult::Unchanged;
///         }
///         render.padding = self.padding;
///         UpdateResult::NeedsLayout
///     }
/// }
/// ```
pub trait RenderView<P: Protocol, A: Arity>: Send + Sync + 'static {
    /// Associated render object type.
    type RenderObject: RenderObjectFor<P, A>;

    /// Create render object from this view configuration.
    ///
    /// Called once when element is first mounted.
    fn create(&self) -> Self::RenderObject;

    /// Update render object when view configuration changes.
    ///
    /// Returns what kind of update is needed:
    /// - `Unchanged` - nothing changed, skip work
    /// - `NeedsLayout` - layout-affecting properties changed
    /// - `NeedsPaint` - only visual properties changed
    ///
    /// Default: Returns `Unchanged` (immutable render object).
    fn update(&self, _render: &mut Self::RenderObject) -> UpdateResult {
        UpdateResult::Unchanged
    }

    /// Cleanup when element is unmounted (optional).
    fn dispose(&self, _render: &mut Self::RenderObject) {}
}

/// Helper trait to constrain RenderObject types.
///
/// Combines `RenderObject` (for layout/paint/hit_test) with protocol/arity constraints.
pub trait RenderObjectFor<P: Protocol, A: Arity>: RenderObject {}

impl<A: Arity, R> RenderObjectFor<BoxProtocol, A> for R where R: RenderBox<A> + RenderObject {}
impl<A: Arity, R> RenderObjectFor<SliverProtocol, A> for R where R: RenderSliver<A> + RenderObject {}

// ============================================================================
// RenderViewConfig - Type-erased config for serialization/debugging
// ============================================================================

/// Type-erased render view configuration.
///
/// Useful for debugging and serialization.
pub trait RenderViewConfig: Send + Sync + Debug {
    /// Debug name for this view.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// ============================================================================
// BUILDER PATTERN EXTENSION
// ============================================================================

/// Extension trait for RenderView with builder methods.
pub trait RenderViewExt: Sized {
    /// Wraps view as a leaf element (no children).
    fn leaf(self) -> RenderViewLeaf<Self>
    where
        Self: RenderView<BoxProtocol, flui_rendering::core::arity::Leaf>,
    {
        RenderViewLeaf { view: self }
    }

    /// Wraps view with a single child.
    fn with_child<C>(self, child: C) -> RenderViewWithChild<Self, C>
    where
        Self: RenderView<BoxProtocol, flui_rendering::core::arity::Single>,
    {
        RenderViewWithChild { view: self, child }
    }

    /// Wraps view with an optional child.
    fn with_optional_child<C>(self, child: Option<C>) -> RenderViewWithOptionalChild<Self, C>
    where
        Self: RenderView<BoxProtocol, flui_rendering::core::arity::Optional>,
    {
        RenderViewWithOptionalChild { view: self, child }
    }

    /// Wraps view with multiple children.
    fn with_children<C>(self, children: Vec<C>) -> RenderViewWithChildren<Self, C>
    where
        Self: RenderView<BoxProtocol, flui_rendering::core::arity::Variable>,
    {
        RenderViewWithChildren {
            view: self,
            children,
        }
    }
}

impl<T> RenderViewExt for T where T: Sized {}

// ============================================================================
// BUILDER WRAPPERS
// ============================================================================

/// Wrapper for leaf render view (no children).
#[derive(Debug, Clone)]
pub struct RenderViewLeaf<V> {
    /// The view configuration
    pub view: V,
}

/// Wrapper for render view with single child.
#[derive(Debug)]
pub struct RenderViewWithChild<V, C> {
    /// The view configuration
    pub view: V,
    /// The single child element
    pub child: C,
}

/// Wrapper for render view with optional child.
#[derive(Debug)]
pub struct RenderViewWithOptionalChild<V, C> {
    /// The view configuration
    pub view: V,
    /// Optional child element
    pub child: Option<C>,
}

/// Wrapper for render view with multiple children.
#[derive(Debug)]
pub struct RenderViewWithChildren<V, C> {
    /// The view configuration
    pub view: V,
    /// Child elements
    pub children: Vec<C>,
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_result_methods() {
        assert!(!UpdateResult::Unchanged.needs_update());
        assert!(UpdateResult::NeedsLayout.needs_update());
        assert!(UpdateResult::NeedsPaint.needs_update());

        assert!(UpdateResult::NeedsLayout.needs_layout());
        assert!(!UpdateResult::NeedsPaint.needs_layout());

        assert!(UpdateResult::NeedsLayout.needs_paint());
        assert!(UpdateResult::NeedsPaint.needs_paint());
        assert!(!UpdateResult::Unchanged.needs_paint());
    }
}
