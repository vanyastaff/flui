//! Render trait - single trait for all renderers
//!
//! This module provides the `Render` trait for implementing renderers
//! with any number of children (0, 1, or multiple).
//!
//! # Architecture
//!
//! - **Single trait** for all renderers (regardless of child count)
//! - **Children enum** to handle all child count patterns
//! - **Context structs** (LayoutContext, PaintContext) for clean API
//! - **Arity validation** at runtime via `arity()` method
//! - **ParentData system** for metadata (stored in RenderElement)
//!
//! # Usage Patterns
//!
//! ## Leaf Render (0 children)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderText {
//!     text: String,
//! }
//!
//! impl Render for RenderText {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         // Compute text size
//!         let size = measure_text(&self.text);
//!         ctx.constraints.constrain(size)
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
//!         let mut layer = pool::acquire_picture();
//!         layer.draw_text(&self.text, ctx.offset);
//!         Box::new(layer)
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(0)  // No children
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//! ```
//!
//! ## Single Child Render
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl Render for RenderPadding {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         let child_id = ctx.children.single();
//!         let deflated = ctx.constraints.deflate(&self.padding);
//!         let child_size = ctx.layout_child(child_id, deflated);
//!         Size::new(
//!             child_size.width + self.padding.horizontal_total(),
//!             child_size.height + self.padding.vertical_total(),
//!         )
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
//!         let child_id = ctx.children.single();
//!         let offset = ctx.offset + self.padding.top_left_offset();
//!         ctx.paint_child(child_id, offset)
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Exact(1)  // Exactly one child
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//! ```
//!
//! ## Multiple Children Render
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderColumn {
//!     spacing: f32,
//! }
//!
//! impl Render for RenderColumn {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size {
//!         let mut y = 0.0;
//!         let mut max_width = 0.0;
//!
//!         for &child_id in ctx.children.as_slice() {
//!             let child_size = ctx.layout_child(child_id, ctx.constraints);
//!             y += child_size.height + self.spacing;
//!             max_width = max_width.max(child_size.width);
//!         }
//!
//!         Size::new(max_width, y - self.spacing)
//!     }
//!
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
//!         let mut container = pool::acquire_container();
//!         let mut y = 0.0;
//!
//!         for &child_id in ctx.children.as_slice() {
//!             let offset = Offset::new(0.0, y);
//!             container.child(ctx.paint_child(child_id, ctx.offset + offset));
//!             y += child_sizes[i].height + self.spacing;
//!         }
//!
//!         Box::new(container)
//!     }
//!
//!     fn arity(&self) -> Arity {
//!         Arity::Variable  // Any number of children
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any {
//!         self
//!     }
//! }
//! ```

use crate::render::{Arity, LayoutContext, PaintContext};
use flui_engine::BoxedLayer;
use flui_types::Size;
use std::fmt::Debug;

/// Render trait for all renderers
///
/// The Render trait is FLUI's abstraction for layout and painting. It's the
/// final layer in the three-tree architecture, responsible for computing sizes
/// and generating the visual output.
///
/// # What is a Renderer?
///
/// Similar to:
/// - **Flutter**: RenderObject (handles layout and paint)
/// - **SwiftUI**: Layout protocol (computes sizes and positions)
/// - **DOM**: Layout engine (computes box model)
///
/// # Three Render Patterns
///
/// FLUI supports three patterns based on child count:
///
/// | Pattern | Children | Arity | Example |
/// |---------|----------|-------|---------|
/// | **Leaf** | 0 | `Arity::Exact(0)` | Text, Image, Box |
/// | **Single** | 1 | `Arity::Exact(1)` | Padding, Opacity, Transform |
/// | **Multi** | N | `Arity::Variable` | Column, Row, Stack |
///
/// All three patterns use the same `Render` trait - just differ in how they
/// access children via `LayoutContext` and `PaintContext`.
///
/// # Required Methods
///
/// 1. **`layout`**: Compute size given constraints
///    - Input: `LayoutContext` (contains constraints and children)
///    - Output: `Size` (computed size)
///    - Side effects: Updates children's sizes via `ctx.layout_child()`
///
/// 2. **`paint`**: Generate layer tree for rendering
///    - Input: `PaintContext` (contains offset and children)
///    - Output: `BoxedLayer` (layer tree for GPU)
///    - Side effects: Paints children via `ctx.paint_child()`
///
/// 3. **`as_any`**: Enable downcasting for metadata access
///    - Required for type-safe metadata (e.g., FlexFit for Flexible)
///
/// 4. **`arity`**: Specify expected child count
///    - Default: `Arity::Variable` (any number of children)
///    - Override with `Arity::Exact(n)` for strict validation
///
/// # Optional Methods
///
/// - `intrinsic_width`: Compute intrinsic width (for sizing)
/// - `intrinsic_height`: Compute intrinsic height (for sizing)
/// - `debug_name`: Get debug name for diagnostics
///
/// # Thread Safety
///
/// All renderers must be `Send + Sync + 'static`:
/// - **`Send`**: Can be moved between threads
/// - **`Sync`**: Can be accessed concurrently from multiple threads
/// - **`'static`**: No borrowed data (owns all state)
///
/// This enables parallel layout and concurrent rendering.
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
    /// of this renderer given the constraints from its parent.
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
    /// tree for this renderer and its children.
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
    /// Returns the arity specification for this renderer.
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
    /// Returns the intrinsic width of this renderer given an optional height.
    /// Used by parent layouts to determine natural sizing.
    ///
    /// # Parameters
    ///
    /// - `height`: Optional height constraint
    ///
    /// # Returns
    ///
    /// - `Some(width)` if this renderer has an intrinsic width
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
    /// Returns the intrinsic height of this renderer given an optional width.
    /// Used by parent layouts to determine natural sizing.
    ///
    /// # Parameters
    ///
    /// - `width`: Optional width constraint
    ///
    /// # Returns
    ///
    /// - `Some(height)` if this renderer has an intrinsic height
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
    /// Allows parent renderers to downcast children to access metadata.
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
    use flui_engine::ContainerLayer;

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

        fn as_any(&self) -> &dyn std::any::Any {
            self
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

        fn as_any(&self) -> &dyn std::any::Any {
            self
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

        fn as_any(&self) -> &dyn std::any::Any {
            self
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
