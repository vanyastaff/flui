//! Unified Render trait - single trait for all render objects
//!
//! This module provides the new unified `Render` trait that replaces
//! the three-trait system (LeafRender, SingleRender, MultiRender).
//!
//! # Architecture
//!
//! - **Single trait** instead of three (LeafRender, SingleRender, MultiRender)
//! - **Children enum** to handle all child count patterns
//! - **Context structs** (LayoutContext, PaintContext) for clean API
//! - **Arity validation** at runtime via `arity()` method
//! - **ParentData system** for metadata (stored in RenderElement)
//!
//! # Migration from Old API
//!
//! ## LeafRender → Render
//!
//! ```rust,ignore
//! // Old API
//! impl LeafRender for RenderParagraph {
//!     type Metadata = ();
//!
//!     fn layout(&mut self, constraints: BoxConstraints) -> Size {
//!         // ...
//!     }
//!
//!     fn paint(&self, offset: Offset) -> BoxedLayer {
//!         // ...
//!     }
//! }
//!
//! // New API
//! impl Render for RenderParagraph {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         // Access constraints via ctx.constraints
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
//!         // Access offset via ctx.offset
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(0)  // No children
//!     }
//! }
//! ```
//!
//! ## SingleRender → Render
//!
//! ```rust,ignore
//! // Old API
//! impl SingleRender for RenderPadding {
//!     type Metadata = ();
//!
//!     fn layout(&mut self, tree: &ElementTree, child_id: ElementId,
//!               constraints: BoxConstraints) -> Size {
//!         let child_size = tree.layout_child(child_id, constraints);
//!         // ...
//!     }
//!
//!     fn paint(&self, tree: &ElementTree, child_id: ElementId,
//!              offset: Offset) -> BoxedLayer {
//!         tree.paint_child(child_id, offset)
//!     }
//! }
//!
//! // New API
//! impl Render for RenderPadding {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         let child_id = ctx.children.single();
//!         let child_size = ctx.layout_child(child_id, ctx.constraints);
//!         // ...
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
//!         let child_id = ctx.children.single();
//!         ctx.paint_child(child_id, ctx.offset)
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(1)  // Exactly one child
//!     }
//! }
//! ```
//!
//! ## MultiRender → Render
//!
//! ```rust,ignore
//! // Old API
//! impl MultiRender for RenderFlex {
//!     type Metadata = ();
//!
//!     fn layout(&mut self, tree: &ElementTree, children: &[ElementId],
//!               constraints: BoxConstraints) -> Size {
//!         for &child_id in children {
//!             tree.layout_child(child_id, child_constraints);
//!         }
//!         // ...
//!     }
//!
//!     fn paint(&self, tree: &ElementTree, children: &[ElementId],
//!              offset: Offset) -> BoxedLayer {
//!         // ...
//!     }
//! }
//!
//! // New API
//! impl Render for RenderFlex {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         for &child_id in ctx.children.as_slice() {
//!             ctx.layout_child(child_id, child_constraints);
//!         }
//!         // ...
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
//!         // ...
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Variable  // Any number of children
//!     }
//! }
//! ```

use crate::render::{Arity, LayoutContext, PaintContext};
use flui_engine::BoxedLayer;
use flui_types::Size;
use std::fmt::Debug;

/// Unified render trait for all render objects
///
/// Replaces the three-trait system (LeafRender, SingleRender, MultiRender)
/// with a single trait that handles all child count patterns via the
/// `Children` enum.
///
/// # Required Methods
///
/// - `layout`: Compute size given constraints (via LayoutContext)
/// - `paint`: Generate layer tree (via PaintContext)
///
/// # Optional Methods
///
/// - `arity`: Specify expected child count (default: `Arity::Variable`)
/// - `intrinsic_width`: Compute intrinsic width
/// - `intrinsic_height`: Compute intrinsic height
/// - `debug_name`: Get debug name for diagnostics
///
/// # Thread Safety
///
/// All render objects must be `Send + Sync + 'static` to enable
/// concurrent rendering across threads.
///
/// # Examples
///
/// ## Leaf Render (No Children)
///
/// ```rust,ignore
/// use flui_core::render::{Render, Arity, LayoutContext, PaintContext};
///
/// #[derive(Debug)]
/// struct RenderBox {
///     color: Color,
/// }
///
/// impl Render for RenderBox {
///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
///         // No children - just return size
///         ctx.constraints.biggest()
///     }
///
///     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
///         // Paint colored box
///         let mut layer = pool::acquire_picture();
///         layer.draw_rect(Rect::from_size(self.size), self.color);
///         Box::new(layer)
///     }
///
///     fn arity(&self) -> Arity {
///         Arity::Exact(0)  // No children allowed
///     }
/// }
/// ```
///
/// ## Single Child Render
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderOpacity {
///     opacity: f32,
/// }
///
/// impl Render for RenderOpacity {
///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
///         let child_id = ctx.children.single();
///         ctx.layout_child(child_id, ctx.constraints)
///     }
///
///     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
///         let child_id = ctx.children.single();
///         let child_layer = ctx.paint_child(child_id, ctx.offset);
///         Box::new(OpacityLayer::new(child_layer, self.opacity))
///     }
///
///     fn arity(&self) -> Arity {
///         Arity::Exact(1)  // Exactly one child required
///     }
/// }
/// ```
///
/// ## Multi-Child Render
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderColumn {
///     spacing: f32,
/// }
///
/// impl Render for RenderColumn {
///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
///         let mut total_height = 0.0;
///         let mut max_width = 0.0;
///
///         for &child_id in ctx.children.as_slice() {
///             let child_size = ctx.layout_child(child_id, ctx.constraints);
///             total_height += child_size.height + self.spacing;
///             max_width = max_width.max(child_size.width);
///         }
///
///         Size::new(max_width, total_height)
///     }
///
///     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
///         let mut container = pool::acquire_container();
///         let mut y = 0.0;
///
///         for &child_id in ctx.children.as_slice() {
///             let offset = Offset::new(0.0, y);
///             container.child(ctx.paint_child(child_id, ctx.offset + offset));
///             y += child_size.height + self.spacing;
///         }
///
///         Box::new(container)
///     }
///
///     fn arity(&self) -> Arity {
///         Arity::Variable  // Any number of children
///     }
/// }
/// ```
pub trait Render: Send + Sync + Debug + 'static {
    /// Compute layout with context
    ///
    /// This method is called during the layout phase to compute the size
    /// of this render object given the constraints from its parent.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Layout context providing access to:
    ///   - `ctx.tree`: Element tree for child layout
    ///   - `ctx.children`: Children enum (None/Single/Multi)
    ///   - `ctx.constraints`: Layout constraints from parent
    ///
    /// # Returns
    ///
    /// The computed size (must satisfy `ctx.constraints`).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn layout(&mut self, ctx: &LayoutContext) -> Size {
    ///     // For leaf nodes: compute intrinsic size
    ///     let intrinsic_size = self.compute_size();
    ///     ctx.constraints.constrain(intrinsic_size)
    ///
    ///     // For single child: delegate and wrap
    ///     let child_id = ctx.children.single();
    ///     let child_size = ctx.layout_child(child_id, child_constraints);
    ///     Size::new(child_size.width + padding, child_size.height + padding)
    ///
    ///     // For multiple children: layout all and compute total
    ///     let mut total_size = Size::ZERO;
    ///     for &child_id in ctx.children.as_slice() {
    ///         let child_size = ctx.layout_child(child_id, constraints);
    ///         total_size = total_size + child_size;
    ///     }
    ///     total_size
    /// }
    /// ```
    fn layout(&mut self, ctx: &LayoutContext) -> Size;

    /// Paint with context
    ///
    /// This method is called during the paint phase to generate the layer
    /// tree for this render object and its children.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Paint context providing access to:
    ///   - `ctx.tree`: Element tree for child painting
    ///   - `ctx.children`: Children enum (None/Single/Multi)
    ///   - `ctx.offset`: Paint offset in parent's coordinate space
    ///
    /// # Returns
    ///
    /// A boxed layer containing the painted content.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    ///     // For leaf nodes: paint self
    ///     let mut layer = pool::acquire_picture();
    ///     layer.draw_text(self.text, ctx.offset, self.color);
    ///     Box::new(layer)
    ///
    ///     // For single child: paint child with offset
    ///     let child_id = ctx.children.single();
    ///     ctx.paint_child(child_id, ctx.offset + padding_offset)
    ///
    ///     // For multiple children: paint all into container
    ///     let mut container = pool::acquire_container();
    ///     for (i, &child_id) in ctx.children.as_slice().iter().enumerate() {
    ///         let offset = ctx.offset + self.child_offsets[i];
    ///         container.child(ctx.paint_child(child_id, offset));
    ///     }
    ///     Box::new(container)
    /// }
    /// ```
    fn paint(&self, ctx: &PaintContext) -> BoxedLayer;

    /// Get arity (expected child count)
    ///
    /// Returns the arity specification for this render object.
    /// Used for runtime validation during element mounting.
    ///
    /// # Default Implementation
    ///
    /// Returns `Arity::Variable` (allows any number of children).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Leaf render - no children
    /// fn arity(&self) -> Arity {
    ///     Arity::Exact(0)
    /// }
    ///
    /// // Single child render
    /// fn arity(&self) -> Arity {
    ///     Arity::Exact(1)
    /// }
    ///
    /// // Multi-child render (default)
    /// fn arity(&self) -> Arity {
    ///     Arity::Variable
    /// }
    ///
    /// // Fixed arity (e.g., split pane with exactly 2 children)
    /// fn arity(&self) -> Arity {
    ///     Arity::Exact(2)
    /// }
    /// ```
    fn arity(&self) -> Arity {
        Arity::Variable
    }

    /// Optional: compute intrinsic width
    ///
    /// Returns the intrinsic width of this render object given an optional height.
    /// Used by parent layouts to determine natural sizing.
    ///
    /// # Parameters
    ///
    /// - `height`: Optional height constraint
    ///
    /// # Returns
    ///
    /// - `Some(width)` if this render object has an intrinsic width
    /// - `None` if intrinsic width is undefined (default)
    ///
    /// # Default Implementation
    ///
    /// Returns `None`.
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Optional: compute intrinsic height
    ///
    /// Returns the intrinsic height of this render object given an optional width.
    /// Used by parent layouts to determine natural sizing.
    ///
    /// # Parameters
    ///
    /// - `width`: Optional width constraint
    ///
    /// # Returns
    ///
    /// - `Some(height)` if this render object has an intrinsic height
    /// - `None` if intrinsic height is undefined (default)
    ///
    /// # Default Implementation
    ///
    /// Returns `None`.
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    /// Downcast to Any for metadata access
    ///
    /// Allows parent render objects to downcast children to access metadata.
    /// This is used by layouts like Flex and Stack to query child-specific metadata
    /// (e.g., FlexItemMetadata, PositionedMetadata).
    ///
    /// # Implementation
    ///
    /// All implementations should simply return `self`:
    ///
    /// ```rust,ignore
    /// fn as_any(&self) -> &dyn std::any::Any {
    ///     self
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Parent accessing child metadata
    /// if let Some(flex_item) = child_render.as_any().downcast_ref::<RenderFlexItem>() {
    ///     let flex = flex_item.metadata.flex;
    ///     // Use flex factor...
    /// }
    /// ```
    fn as_any(&self) -> &dyn std::any::Any;

    /// Debug name for diagnostics
    ///
    /// Returns a human-readable name for this render object.
    /// Used in debug output, error messages, and dev tools.
    ///
    /// # Default Implementation
    ///
    /// Returns the type name (e.g., "my_crate::RenderPadding").
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn debug_name(&self) -> &'static str {
    ///     "RenderPadding"
    /// }
    /// ```
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Children;
    use crate::element::ElementTree;
    use flui_engine::ContainerLayer;
    use flui_types::constraints::BoxConstraints;

    #[derive(Debug)]
    struct TestLeafRender;

    impl Render for TestLeafRender {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            ctx.constraints.constrain(Size::new(100.0, 100.0))
        }

        fn paint(&self, _ctx: &PaintContext) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }

        fn arity(&self) -> Arity {
            Arity::Exact(0)
        }
    }

    #[derive(Debug)]
    struct TestSingleRender;

    impl Render for TestSingleRender {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            let child_id = ctx.children.single();
            ctx.layout_child(child_id, ctx.constraints)
        }

        fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
            let child_id = ctx.children.single();
            ctx.paint_child(child_id, ctx.offset)
        }

        fn arity(&self) -> Arity {
            Arity::Exact(1)
        }
    }

    #[derive(Debug)]
    struct TestMultiRender;

    impl Render for TestMultiRender {
        fn layout(&mut self, ctx: &LayoutContext) -> Size {
            ctx.constraints.biggest()
        }

        fn paint(&self, _ctx: &PaintContext) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }

        fn arity(&self) -> Arity {
            Arity::Variable
        }
    }

    #[test]
    fn test_leaf_render_arity() {
        let render = TestLeafRender;
        assert_eq!(render.arity(), Arity::Exact(0));
    }

    #[test]
    fn test_single_render_arity() {
        let render = TestSingleRender;
        assert_eq!(render.arity(), Arity::Exact(1));
    }

    #[test]
    fn test_multi_render_arity() {
        let render = TestMultiRender;
        assert_eq!(render.arity(), Arity::Variable);
    }

    #[test]
    fn test_default_intrinsic_methods() {
        let render = TestLeafRender;
        assert_eq!(render.intrinsic_width(Some(100.0)), None);
        assert_eq!(render.intrinsic_height(Some(100.0)), None);
    }

    #[test]
    fn test_debug_name() {
        let render = TestLeafRender;
        let name = render.debug_name();
        assert!(name.contains("TestLeafRender"));
    }
}
