//! Universal Arity System - Compile-time child count constraints
//!
//! This module provides a universal arity system that can be used across
//! all three trees in the FLUI architecture:
//! - **Widget Tree**: Widgets with LeafArity, SingleArity, or MultiArity
//! - **Element Tree**: Elements inheriting arity from their widgets
//! - **RenderObject Tree**: RenderObjects with typed child access
//!
//! This avoids duplicating arity systems (WidgetArity, ElementArity, RenderArity)
//! and provides a single, consistent type-level constraint mechanism.
//!
//! # Design
//!
//! The `Arity` trait encodes child count constraints at compile time:
//! - `LeafArity`: No children allowed (arity = 0)
//! - `SingleArity`: Exactly one child required (arity = 1)
//! - `MultiArity`: Zero or more children allowed (arity = 0..N)
//!
//! # Benefits
//!
//! 1. **Compile-Time Safety**: Wrong child counts caught at compile time
//! 2. **Zero Runtime Cost**: Arity is a zero-sized type, optimized away
//! 3. **Type-Driven API**: Context types provide different methods based on arity
//! 4. **Universal**: Same arity system across Widget/Element/RenderObject
//!
//! # Example
//!
//! ```rust,ignore
//! // Universal across all tree types
//!
//! // Widget with single child
//! impl Widget for Opacity {
//!     type Arity = SingleArity;
//! }
//!
//! // RenderObject with multiple children
//! impl RenderObject for RenderFlex {
//!     type Arity = MultiArity;
//!
//!     fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
//!         // MultiArity enables .children() method
//!         for child in cx.children() {
//!             cx.layout_child(child, constraints);
//!         }
//!     }
//! }
//!
//! // Element with no children
//! impl Element for ImageElement {
//!     type Arity = LeafArity;
//! }
//! ```

/// Universal marker trait for child count constraints
///
/// This trait encodes arity (child count) at the type level, enabling
/// compile-time verification of child relationships across all three trees.
///
/// # Implementations
///
/// - `LeafArity`: No children (e.g., Text, Image, ColoredBox)
/// - `SingleArity`: Exactly one child (e.g., Opacity, Padding, Transform)
/// - `MultiArity`: Zero or more children (e.g., Flex, Stack, Wrap)
///
/// # Type-Driven API
///
/// The arity type parameter enables different methods on context types:
///
/// ```rust,ignore
/// // LeafArity - no child access
/// fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
///     // cx.child()     ❌ Not available
///     // cx.children()  ❌ Not available
/// }
///
/// // SingleArity - single child access
/// fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
///     let child = cx.child();           // ✅ Available
///     cx.layout_child(child, constraints);
/// }
///
/// // MultiArity - multiple children access
/// fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
///     for child in cx.children() {      // ✅ Available
///         cx.layout_child(child, constraints);
///     }
/// }
/// ```
pub trait Arity: Send + Sync + 'static {
    /// Human-readable name for debugging and error messages
    fn name() -> &'static str;

    /// Compile-time child count constraint
    ///
    /// - `Some(0)` for `LeafArity` (no children)
    /// - `Some(1)` for `SingleArity` (exactly one child)
    /// - `None` for `MultiArity` (variable count, 0..N children)
    const CHILD_COUNT: Option<usize>;
}

/// Leaf arity - no children allowed
///
/// Used for nodes that cannot have children, such as:
/// - Text/Paragraph rendering
/// - Image rendering
/// - Colored boxes
/// - Icons
///
/// # Context Methods
///
/// - `LayoutCx<LeafArity>`: NO child access methods
/// - `PaintCx<LeafArity>`: NO child painting methods
/// - `BuildCx<LeafArity>`: NO child building methods
///
/// # Example
///
/// ```rust,ignore
/// impl RenderObject for RenderParagraph {
///     type Arity = LeafArity;
///
///     fn layout(&mut self, cx: &mut LayoutCx<LeafArity>) -> Size {
///         // No children to layout
///         self.text_layout.size()
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeafArity;

impl Arity for LeafArity {
    fn name() -> &'static str {
        "Leaf"
    }

    const CHILD_COUNT: Option<usize> = Some(0);
}

/// Single arity - exactly one child required
///
/// Used for wrapper nodes that modify a single child, such as:
/// - Opacity/transparency
/// - Padding/margins
/// - Transforms (rotation, scale, translation)
/// - Clipping
/// - Alignment
///
/// # Context Methods
///
/// - `LayoutCx<SingleArity>`: `.child()` and `.layout_child()`
/// - `PaintCx<SingleArity>`: `.child()` and `.capture_child_layer()`
/// - `BuildCx<SingleArity>`: `.child()` and `.build_child()`
///
/// # Example
///
/// ```rust,ignore
/// impl RenderObject for RenderOpacity {
///     type Arity = SingleArity;
///
///     fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
///         let child = cx.child();
///         cx.layout_child(child, constraints)
///     }
///
///     fn paint(&self, cx: &PaintCx<SingleArity>) -> BoxedLayer {
///         let child_layer = cx.capture_child_layer(cx.child());
///         Box::new(OpacityLayer::new(child_layer, self.opacity))
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SingleArity;

impl Arity for SingleArity {
    fn name() -> &'static str {
        "Single"
    }

    const CHILD_COUNT: Option<usize> = Some(1);
}

/// Multi arity - zero or more children allowed
///
/// Used for container nodes that can have multiple children, such as:
/// - Flex layouts (Row, Column)
/// - Stack layouts (z-ordering)
/// - Wrap layouts
/// - Grid layouts
/// - Custom multi-child layouts
///
/// # Context Methods
///
/// - `LayoutCx<MultiArity>`: `.children()` and `.layout_child()`
/// - `PaintCx<MultiArity>`: `.children()` and `.capture_child_layers()`
/// - `BuildCx<MultiArity>`: `.children()` and `.build_children()`
///
/// # Example
///
/// ```rust,ignore
/// impl RenderObject for RenderFlex {
///     type Arity = MultiArity;
///
///     fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
///         let mut total_size = 0.0;
///         for &child in cx.children() {
///             let child_size = cx.layout_child(child, constraints);
///             total_size += child_size.width;
///         }
///         Size::new(total_size, constraints.max_height)
///     }
///
///     fn paint(&self, cx: &PaintCx<MultiArity>) -> BoxedLayer {
///         let layers = cx.capture_child_layers();
///         Box::new(ContainerLayer::new_with_children(layers))
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultiArity;

impl Arity for MultiArity {
    fn name() -> &'static str {
        "Multi"
    }

    const CHILD_COUNT: Option<usize> = None; // Variable count (0..N)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arity_names() {
        assert_eq!(LeafArity::name(), "Leaf");
        assert_eq!(SingleArity::name(), "Single");
        assert_eq!(MultiArity::name(), "Multi");
    }

    #[test]
    fn test_child_counts() {
        assert_eq!(LeafArity::CHILD_COUNT, Some(0));
        assert_eq!(SingleArity::CHILD_COUNT, Some(1));
        assert_eq!(MultiArity::CHILD_COUNT, None);
    }

    #[test]
    fn test_arity_is_zero_sized() {
        use std::mem::size_of;

        // Arity types should be zero-sized (no runtime overhead)
        assert_eq!(size_of::<LeafArity>(), 0);
        assert_eq!(size_of::<SingleArity>(), 0);
        assert_eq!(size_of::<MultiArity>(), 0);
    }
}
