//! Render view trait for creating render objects.

use crate::element::{Element, IntoElement};
use crate::render::arity::Arity;
use crate::render::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::render::{RenderBox, SliverRender};
use crate::view::UpdateResult;
use std::fmt::Debug;

// ============================================================================
// RENDER VIEW TRAIT
// ============================================================================

/// Render view - widget that creates render objects.
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
/// #[derive(Clone)]
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
///
/// // Usage:
/// Padding {
///     padding: EdgeInsets::all(16.0),
///     child: Text::new("Hello").into(),
/// }
/// ```
pub trait RenderView<P: Protocol, A: Arity>: Clone + Send + 'static {
    /// Associated render object type.
    type RenderObject: RenderObjectFor<P, A>;

    /// Create render object from this view configuration.
    ///
    /// Called once when element is first mounted.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn create(&self) -> RenderPadding {
    ///     RenderPadding::new(self.padding)
    /// }
    /// ```
    fn create(&self) -> Self::RenderObject;

    /// Update render object when view configuration changes.
    ///
    /// Returns what kind of update is needed:
    /// - `Unchanged` - nothing changed, skip work
    /// - `NeedsLayout` - layout-affecting properties changed
    /// - `NeedsPaint` - only visual properties changed
    ///
    /// # Default
    ///
    /// Returns `Unchanged` (immutable render object).
    ///
    /// # Example: Layout change
    ///
    /// ```rust,ignore
    /// fn update(&self, render: &mut RenderPadding) -> UpdateResult {
    ///     if render.padding == self.padding {
    ///         return UpdateResult::Unchanged;
    ///     }
    ///     render.padding = self.padding;
    ///     UpdateResult::NeedsLayout
    /// }
    /// ```
    ///
    /// # Example: Paint-only change
    ///
    /// ```rust,ignore
    /// fn update(&self, render: &mut RenderOpacity) -> UpdateResult {
    ///     if render.opacity == self.opacity {
    ///         return UpdateResult::Unchanged;
    ///     }
    ///     render.opacity = self.opacity;
    ///     UpdateResult::NeedsPaint  // Doesn't affect layout!
    /// }
    /// ```
    fn update(&self, render: &mut Self::RenderObject) -> UpdateResult {
        let _ = render;
        UpdateResult::Unchanged
    }

    /// Cleanup when element is unmounted (optional).
    ///
    /// Override to dispose resources held by render object.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn dispose(&self, render: &mut RenderImage) {
    ///     render.dispose_texture();
    /// }
    /// ```
    fn dispose(&self, render: &mut Self::RenderObject) {
        let _ = render;
    }
}

// ============================================================================
// HELPER TRAIT
// ============================================================================

/// Helper trait to constrain RenderObject types.
pub trait RenderObjectFor<P: Protocol, A: Arity>: Send + Sync + Debug + 'static {}

impl<A: Arity, R> RenderObjectFor<BoxProtocol, A> for R where R: RenderBox<A> {}
impl<A: Arity, R> RenderObjectFor<SliverProtocol, A> for R where R: SliverRender<A> {}

// ============================================================================
// BUILDER PATTERN EXTENSION (modern unified architecture)
// ============================================================================

/// Extension trait for RenderView with builder methods.
///
/// **Note:** These builders work with the unified Element + ViewObject architecture.
/// They create RenderViewWrapper instances that delegate to the underlying RenderView.
pub trait RenderViewExt: Sized {
    /// Wraps view as a leaf element (no children).
    fn leaf(self) -> RenderViewLeaf<Self>
    where
        Self: RenderView<BoxProtocol, crate::render::arity::Leaf>,
    {
        RenderViewLeaf { view: self }
    }

    /// Wraps view with a single child.
    fn child<C: IntoElement>(self, child: C) -> RenderViewWithChild<Self, C>
    where
        Self: RenderView<BoxProtocol, crate::render::arity::Single>,
    {
        RenderViewWithChild { view: self, child }
    }

    /// Wraps view with an optional child.
    fn child_opt<C: IntoElement>(self, child: Option<C>) -> RenderViewWithOptionalChild<Self, C>
    where
        Self: RenderView<BoxProtocol, crate::render::arity::Optional>,
    {
        RenderViewWithOptionalChild { view: self, child }
    }

    /// Wraps view with multiple children.
    fn children(self, children: impl Into<Vec<Element>>) -> RenderViewWithChildren<Self>
    where
        Self: RenderView<BoxProtocol, crate::render::arity::Variable>,
    {
        RenderViewWithChildren {
            view: self,
            children: children.into(),
        }
    }
}

/// Auto-implement for all types (blanket impl)
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
#[derive(Debug, Clone)]
pub struct RenderViewWithChild<V, C> {
    /// The view configuration
    pub view: V,
    /// The single child element
    pub child: C,
}

/// Wrapper for render view with optional child.
#[derive(Debug, Clone)]
pub struct RenderViewWithOptionalChild<V, C> {
    /// The view configuration
    pub view: V,
    /// Optional child element
    pub child: Option<C>,
}

/// Wrapper for render view with multiple children.
#[derive(Debug)]
pub struct RenderViewWithChildren<V> {
    /// The view configuration
    pub view: V,
    /// Child elements
    pub children: Vec<Element>,
}

// ============================================================================
// INTO ELEMENT IMPLEMENTATIONS
// ============================================================================

// Note: RenderView types are converted to Elements through ViewObject wrappers
// (RenderViewWrapper), not directly through IntoElement. This allows proper
// lifecycle management and type erasure.

// ============================================================================
// CONVENIENCE TYPE ALIASES
// ============================================================================

/// Leaf box render view
pub type LeafBoxRenderView = dyn RenderView<BoxProtocol, crate::render::arity::Leaf>;

/// Single-child box render view
pub type SingleChildBoxRenderView = dyn RenderView<BoxProtocol, crate::render::arity::Single>;

/// Multi-child box render view
pub type MultiChildBoxRenderView = dyn RenderView<BoxProtocol, crate::render::arity::Variable>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::arity::{Leaf, Single};
    use crate::render::contexts::{LayoutContext, PaintContext};

    #[derive(Clone, Debug)]
    struct MockLeafView;

    #[derive(Debug)]
    struct MockLeafRender;

    impl RenderBox<Leaf> for MockLeafRender {
        fn layout(
            &mut self,
            _ctx: LayoutContext<'_, Leaf, BoxProtocol>,
        ) -> crate::render::protocol::Size {
            crate::render::protocol::Size::new(100.0, 100.0)
        }

        fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {}
    }

    impl RenderView<BoxProtocol, Leaf> for MockLeafView {
        type RenderObject = MockLeafRender;

        fn create(&self) -> MockLeafRender {
            MockLeafRender
        }
    }

    #[test]
    fn test_render_view_leaf() {
        let view = MockLeafView;
        let _wrapper = view.leaf();
    }

    #[test]
    fn test_render_view_create() {
        let view = MockLeafView;
        let _render = view.create();
    }
}
