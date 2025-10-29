//! Render traits for enum-based architecture
//!
//! This module defines three object-safe traits:
//! - LeafRender: For leaf nodes with no children
//! - SingleRender: For nodes with exactly one child
//! - MultiRender: For nodes with multiple children
//!
//! These traits are object-safe and have simple, clean APIs.

use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

use crate::element::{ElementId, ElementTree};

// ========== Leaf Render ==========

/// Trait for leaf render objects (no children)
///
/// Leaf render objects are terminal nodes that handle their own rendering
/// without delegating to children.
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
    /// Compute layout
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint to layer
    fn paint(&self, offset: Offset) -> BoxedLayer;

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

// ========== Single Render ==========

/// Trait for single-child render objects
///
/// Single-child render objects wrap exactly one child and provide
/// transformation or effects (e.g., padding, opacity, clipping).
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
    /// Compute layout
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size;

    /// Paint to layer
    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer;

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

// ========== Multi Render ==========

/// Trait for multi-child render objects
///
/// Multi-child render objects arrange multiple children according to
/// layout algorithms (e.g., flex, stack, grid).
///
/// # Examples
///
/// - Row/Column (flex)
/// - Stack (layered)
/// - Wrap (wrapping)
/// - Grid
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
///     fn layout(
///         &mut self,
///         tree: &ElementTree,
///         children: &[ElementId],
///         constraints: BoxConstraints,
///     ) -> Size {
///         let mut total_size = Size::ZERO;
///         for &child in children {
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
    /// Compute layout
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size;

    /// Paint to layer
    fn paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> BoxedLayer;

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

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    #[derive(Debug)]
    struct TestLeaf;

    impl LeafRender for TestLeaf {
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
        fn layout(
            &mut self,
            _tree: &ElementTree,
            _child_id: ElementId,
            constraints: BoxConstraints,
        ) -> Size {
            constraints.constrain(Size::new(200.0, 200.0))
        }

        fn paint(
            &self,
            _tree: &ElementTree,
            _child_id: ElementId,
            _offset: Offset,
        ) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug)]
    struct TestMulti;

    impl MultiRender for TestMulti {
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
        let _leaf: Box<dyn LeafRender> = Box::new(TestLeaf);
        let _single: Box<dyn SingleRender> = Box::new(TestSingle);
        let _multi: Box<dyn MultiRender> = Box::new(TestMulti);
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
