//! Render traits for enum-based architecture
//!
//! This module defines three object-safe traits:
//! - LeafRender: For leaf nodes with no children
//! - SingleRender: For nodes with exactly one child
//! - MultiRender: For nodes with multiple children
//!
//! These traits are object-safe and have simple, clean APIs.

use flui_engine::BoxedLayer;
use flui_types::{constraints::BoxConstraints, Offset, Size};

use crate::element::ElementId;
use crate::element::ElementTree;

// ========== Leaf Render ==========

/// Trait for leaf render objects (no children)
///
/// Leaf render objects are terminal nodes that handle their own rendering
/// without delegating to children.
///
/// # Generic Associated Type (GAT): Metadata
///
/// Each render object can define its own `Metadata` type for storing
/// custom data. Use `()` if no metadata is needed (zero-cost).
///
/// ```rust,ignore
/// // Example: No metadata needed
/// impl LeafRender for RenderText {
///     type Metadata = ();  // Default, zero-cost
/// }
///
/// // Example: With metadata (e.g., for parent's layout algorithm)
/// impl LeafRender for RenderFlexItem {
///     type Metadata = FlexItemMetadata;
///
///     fn metadata(&self) -> Option<&dyn std::any::Any> {
///         Some(&self.flex_metadata)
///     }
/// }
/// ```
///
/// # Examples
///
/// - Text rendering
/// - Image rendering
/// - Placeholder boxes
/// - Custom painters
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::render::LeafRender;
/// use flui_engine::{BoxedLayer, PictureLayer};
///
/// #[derive(Debug)]
/// struct Paragraph {
///     text: String,
/// }
///
/// impl LeafRender for Paragraph {
///     type ParentData = ();  // No parent data needed
///
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         let width = self.text.len() as f32 * 10.0;
///         constraints.constrain(Size::new(width, 20.0))
///     }
///
///     fn paint(&self, offset: Offset) -> BoxedLayer {
///         let mut layer = PictureLayer::new();
///         // Draw text at offset...
///         Box::new(layer)
///     }
/// }
/// ```
pub trait LeafRender: Send + Sync + std::fmt::Debug + 'static {
    /// Associated metadata type.
    ///
    /// Use `()` if this render object doesn't need metadata (zero-cost).
    /// Use a custom type (e.g., `FlexItemMetadata`) if you need to store
    /// additional data for this render object.
    ///
    /// # Zero-cost when unused
    ///
    /// When `Metadata = ()`, there is zero runtime overhead - the compiler
    /// optimizes it away completely.
    type Metadata: std::any::Any + Send + Sync + 'static;

    /// Compute layout
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint to layer
    fn paint(&self, offset: Offset) -> BoxedLayer;

    /// Get metadata (type-erased)
    ///
    /// Returns `Some(&dyn Any)` if this render object has metadata,
    /// `None` otherwise. Caller can downcast to `Self::Metadata`.
    ///
    /// # Default implementation
    ///
    /// Returns `None` by default. Override if metadata is used.
    fn metadata(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Optional: compute intrinsic width
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Optional: compute intrinsic height
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    /// Debug name
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// Note: impl_downcast! cannot be used with GAT traits.
// Downcast functionality is provided via metadata() method which returns &dyn Any.

// ========== Single Render ==========

/// Trait for single-child render objects
///
/// Single-child render objects wrap exactly one child and provide
/// transformation or effects (e.g., padding, opacity, clipping).
///
/// # Generic Associated Type (GAT): Metadata
///
/// Each render object can define its own `ParentData` type for metadata
/// that the parent needs during layout. Use `()` if no parent data is needed.
///
/// # Examples
///
/// - Padding
/// - Center
/// - Opacity
/// - Transform
/// - Clip
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::render::SingleRender;
/// use flui_engine::{BoxedLayer, OpacityLayer};
///
/// #[derive(Debug)]
/// struct Opacity {
///     opacity: f32,
/// }
///
/// impl SingleRender for Opacity {
///     type Metadata = ();  // No metadata needed
///
///     fn layout(
///         &mut self,
///         tree: &ElementTree,
///         child_id: ElementId,
///         constraints: BoxConstraints,
///     ) -> Size {
///         // Layout child and return its size
///         constraints.constrain(Size::new(100.0, 100.0))
///     }
///
///     fn paint(
///         &self,
///         tree: &ElementTree,
///         child_id: ElementId,
///         offset: Offset,
///     ) -> BoxedLayer {
///         let child_layer = Box::new(ContainerLayer::new());
///         Box::new(OpacityLayer::new(child_layer, self.opacity))
///     }
/// }
/// ```
pub trait SingleRender: Send + Sync + std::fmt::Debug + 'static {
    /// Associated metadata type.
    ///
    /// Use `()` if this render object doesn't need metadata (zero-cost).
    /// Use a custom type if you need to store additional data.
    type Metadata: std::any::Any + Send + Sync + 'static;

    /// Compute layout
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size;

    /// Paint to layer
    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer;

    /// Get metadata (type-erased)
    ///
    /// Returns `Some(&dyn Any)` if this render object has parent data,
    /// `None` otherwise. Parent can downcast to `Self::ParentData`.
    fn metadata(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Optional: compute intrinsic width
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Optional: compute intrinsic height
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    /// Debug name
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// Note: impl_downcast! cannot be used with GAT traits.
// Downcast functionality is provided via metadata() method which returns &dyn Any.

// ========== Multi Render ==========

/// Trait for multi-child render objects
///
/// Multi-child render objects arrange multiple children according to
/// layout algorithms (e.g., flex, stack, grid).
///
/// # Generic Associated Type (GAT): Metadata
///
/// Each render object can define its own `Metadata` type for storing
/// custom data. Use `()` if no metadata is needed (zero-cost).
///
/// **Common use case:** Multi-child renders often need per-child metadata
/// (e.g., flex factor, stack position). Children can provide this via their
/// own `Metadata` type.
///
/// # Examples
///
/// - Row/Column (flex) - needs FlexParentData per child
/// - Stack (layered) - needs StackParentData per child
/// - Wrap (wrapping) - may need WrapParentData per child
/// - Grid - needs GridParentData per child
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::render::MultiRender;
/// use flui_engine::{BoxedLayer, ContainerLayer};
///
/// #[derive(Debug)]
/// struct Flex {
///     direction: Axis,
/// }
///
/// impl MultiRender for Flex {
///     type Metadata = ();  // Flex doesn't have metadata itself
///
///     fn layout(
///         &mut self,
///         tree: &ElementTree,
///         children: &[ElementId],
///         constraints: BoxConstraints,
///     ) -> Size {
///         let mut total_size = Size::ZERO;
///         for &child in children {
///             // Can access child's metadata via tree
///             total_size.width += 100.0;
///         }
///         constraints.constrain(total_size)
///     }
///
///     fn paint(
///         &self,
///         tree: &ElementTree,
///         children: &[ElementId],
///         offset: Offset,
///     ) -> BoxedLayer {
///         let mut container = ContainerLayer::new();
///         // Add child layers...
///         Box::new(container)
///     }
/// }
/// ```
pub trait MultiRender: Send + Sync + std::fmt::Debug + 'static {
    /// Associated metadata type.
    ///
    /// Use `()` if this render object doesn't need metadata (zero-cost).
    /// Use a custom type if you need to store additional data.
    type Metadata: std::any::Any + Send + Sync + 'static;

    /// Compute layout
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size;

    /// Paint to layer
    fn paint(&self, tree: &ElementTree, children: &[ElementId], offset: Offset) -> BoxedLayer;

    /// Get metadata (type-erased)
    ///
    /// Returns `Some(&dyn Any)` if this render object has parent data,
    /// `None` otherwise. Parent can downcast to `Self::ParentData`.
    fn metadata(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Optional: compute intrinsic width
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Optional: compute intrinsic height
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    /// Debug name
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// Note: impl_downcast! cannot be used with GAT traits.
// Downcast functionality is provided via metadata() method which returns &dyn Any.

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    #[derive(Debug)]
    struct TestLeaf;

    impl LeafRender for TestLeaf {
        type Metadata = (); // No metadata needed

        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            constraints.constrain(Size::new(100.0, 100.0))
        }

        fn paint(&self, _offset: Offset) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestSingle;

    impl SingleRender for TestSingle {
        type Metadata = ();

        fn layout(
            &mut self,
            _tree: &ElementTree,
            _child_id: ElementId,
            constraints: BoxConstraints,
        ) -> Size {
            constraints.constrain(Size::new(200.0, 200.0))
        }

        fn paint(&self, _tree: &ElementTree, _child_id: ElementId, _offset: Offset) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestMulti;

    impl MultiRender for TestMulti {
        type Metadata = ();

        fn layout(
            &mut self,
            _tree: &ElementTree,
            _children: &[ElementId],
            constraints: BoxConstraints,
        ) -> Size {
            constraints.constrain(Size::new(300.0, 300.0))
        }

        fn paint(
            &self,
            _tree: &ElementTree,
            _children: &[ElementId],
            _offset: Offset,
        ) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_traits_are_object_safe() {
        let _leaf: Box<dyn LeafRender<Metadata = ()>> = Box::new(TestLeaf);
        let _single: Box<dyn SingleRender<Metadata = ()>> = Box::new(TestSingle);
        let _multi: Box<dyn MultiRender<Metadata = ()>> = Box::new(TestMulti);
    }

    #[test]
    fn test_debug_names() {
        let leaf = TestLeaf;
        let single = TestSingle;
        let multi = TestMulti;

        assert!(leaf.debug_name().contains("TestLeaf"));
        assert!(single.debug_name().contains("TestSingle"));
        assert!(multi.debug_name().contains("TestMulti"));
    }
}
