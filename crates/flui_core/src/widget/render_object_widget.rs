//! RenderObjectWidget - widgets that create render objects
//!
//! RenderObjectWidget creates widgets that directly participate in
//! layout and painting. These are the "primitives" of the UI system.
//!
//! # When to Use
//!
//! - Widget needs custom layout logic
//! - Widget needs custom painting
//! - Widget is a layout container (Row, Column, Stack, etc.)
//! - Widget wraps platform views or native controls
//!
//! # Architecture
//!
//! ```text
//! RenderObjectWidget (immutable config)
//!   ↓ creates
//! RenderObject (mutable, performs layout/paint)
//!   ↓
//! Constraints → Layout → Size → Paint
//! ```
//!
//! # Examples
//!
//! ```
//! use flui_core::{RenderObjectWidget, RenderObject};
//!
//! #[derive(Debug)]
//! struct Container {
//!     width: Option<f64>,
//!     height: Option<f64>,
//!     color: Color,
//! }
//!
//! impl RenderObjectWidget for Container {
//!     type RenderObject = RenderContainer;
//!
//!     fn create_render_object(&self) -> Self::RenderObject {
//!         RenderContainer {
//!             width: self.width,
//!             height: self.height,
//!             color: self.color,
//!         }
//!     }
//!
//!     fn update_render_object(&self, render_object: &mut Self::RenderObject) {
//!         render_object.width = self.width;
//!         render_object.height = self.height;
//!         render_object.color = self.color;
//!     }
//! }
//!
//! // Widget and DynWidget are automatic!
//! ```

use crate::RenderObject;
use std::fmt;

/// RenderObjectWidget - widget that creates a render object
///
/// This is the trait for widgets that directly participate in layout
/// and painting by creating RenderObject instances.
///
/// # Separation of Concerns
///
/// - **Widget**: Immutable configuration (what to render)
/// - **RenderObject**: Mutable state (how to render)
///
/// ```text
/// Container{width:100} → RenderContainer{width:100, size:Size{100,50}}
///                               ↓ layout
///                        Computes size, positions children
///                               ↓ paint
///                        Draws to canvas
/// ```
///
/// # Types of RenderObjectWidgets
///
/// ## Single Child
///
/// Widgets with exactly one child (e.g., Padding, Center, SizedBox)
///
/// ```rust
/// impl RenderObjectWidget for Padding {
///     type RenderObject = RenderPadding;
///     type Arity = SingleArity;  // One child
/// }
/// ```
///
/// ## Multi Child
///
/// Widgets with multiple children (e.g., Row, Column, Stack)
///
/// ```rust
/// impl RenderObjectWidget for Column {
///     type RenderObject = RenderColumn;
///     type Arity = MultiArity;  // Multiple children
/// }
/// ```
///
/// ## Leaf
///
/// Widgets with no children (e.g., Text, Image)
///
/// ```rust
/// impl RenderObjectWidget for Text {
///     type RenderObject = RenderParagraph;
///     // Arity = LeafArity (default)
/// }
/// ```
///
/// # Lifecycle
///
/// ```text
/// 1. Widget created: Container { width: 100 }
/// 2. create_render_object() → RenderContainer
/// 3. RenderObject attached to tree
/// 4. Layout pass → RenderObject.layout()
/// 5. Paint pass → RenderObject.paint()
/// 6. Widget updated: Container { width: 200 }
/// 7. update_render_object() → Updates existing RenderContainer
/// 8. Layout + Paint again
/// ...
/// N. Widget removed → RenderObject detached
/// ```
///
/// # Performance
///
/// - **RenderObject persists** - Not recreated on rebuild
/// - **Update is fast** - Only changed properties updated
/// - **Layout caching** - RenderObject caches layout results
/// - **Repaint regions** - Only dirty regions repainted
///
/// # Examples
///
/// ## Single Child Widget (Padding)
///
/// ```
/// use flui_core::{RenderObjectWidget, SingleArity};
///
/// #[derive(Debug)]
/// struct Padding {
///     padding: EdgeInsets,
///     child: BoxedWidget,
/// }
///
/// impl RenderObjectWidget for Padding {
///     type RenderObject = RenderPadding;
///     type Arity = SingleArity;
///
///     fn create_render_object(&self) -> Self::RenderObject {
///         RenderPadding {
///             padding: self.padding,
///         }
///     }
///
///     fn update_render_object(&self, render_object: &mut Self::RenderObject) {
///         render_object.padding = self.padding;
///         if render_object.padding != self.padding {
///             render_object.mark_needs_layout();
///         }
///     }
/// }
///
/// struct RenderPadding {
///     padding: EdgeInsets,
/// }
///
/// impl RenderObject for RenderPadding {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Add padding to child constraints
///         let child_constraints = constraints.deflate(self.padding);
///         let child_size = self.layout_child(child_constraints);
///         child_size.inflate(self.padding)
///     }
///
///     fn paint(&self, context: &mut PaintContext) {
///         // Paint child with offset
///         context.paint_child_with_offset(
///             Offset::new(self.padding.left, self.padding.top)
///         );
///     }
/// }
/// ```
///
/// ## Multi Child Widget (Row)
///
/// ```
/// #[derive(Debug)]
/// struct Row {
///     children: Vec<BoxedWidget>,
///     main_axis_alignment: MainAxisAlignment,
///     cross_axis_alignment: CrossAxisAlignment,
/// }
///
/// impl RenderObjectWidget for Row {
///     type RenderObject = RenderFlex;
///     type Arity = MultiArity;
///
///     fn create_render_object(&self) -> Self::RenderObject {
///         RenderFlex {
///             direction: Axis::Horizontal,
///             main_axis_alignment: self.main_axis_alignment,
///             cross_axis_alignment: self.cross_axis_alignment,
///         }
///     }
///
///     fn update_render_object(&self, render_object: &mut Self::RenderObject) {
///         let mut needs_layout = false;
///
///         if render_object.main_axis_alignment != self.main_axis_alignment {
///             render_object.main_axis_alignment = self.main_axis_alignment;
///             needs_layout = true;
///         }
///
///         if render_object.cross_axis_alignment != self.cross_axis_alignment {
///             render_object.cross_axis_alignment = self.cross_axis_alignment;
///             needs_layout = true;
///         }
///
///         if needs_layout {
///             render_object.mark_needs_layout();
///         }
///     }
/// }
/// ```
///
/// ## Leaf Widget (Text)
///
/// ```
/// #[derive(Debug)]
/// struct Text {
///     content: String,
///     style: TextStyle,
/// }
///
/// impl RenderObjectWidget for Text {
///     type RenderObject = RenderParagraph;
///     // Arity = LeafArity (default)
///
///     fn create_render_object(&self) -> Self::RenderObject {
///         RenderParagraph::new(&self.content, &self.style)
///     }
///
///     fn update_render_object(&self, render_object: &mut Self::RenderObject) {
///         let mut needs_layout = false;
///
///         if render_object.text != self.content {
///             render_object.set_text(&self.content);
///             needs_layout = true;
///         }
///
///         if render_object.style != self.style {
///             render_object.set_style(&self.style);
///             needs_layout = true;
///         }
///
///         if needs_layout {
///             render_object.mark_needs_layout();
///         }
///     }
/// }
/// ```
///
/// ## Custom Painting (Canvas)
///
/// ```
/// #[derive(Debug)]
/// struct CustomPaint {
///     painter: Arc<dyn CustomPainter>,
/// }
///
/// impl RenderObjectWidget for CustomPaint {
///     type RenderObject = RenderCustomPaint;
///
///     fn create_render_object(&self) -> Self::RenderObject {
///         RenderCustomPaint {
///             painter: self.painter.clone(),
///         }
///     }
///
///     fn update_render_object(&self, render_object: &mut Self::RenderObject) {
///         if !Arc::ptr_eq(&render_object.painter, &self.painter) {
///             render_object.painter = self.painter.clone();
///             render_object.mark_needs_paint();
///         }
///     }
/// }
///
/// struct RenderCustomPaint {
///     painter: Arc<dyn CustomPainter>,
/// }
///
/// impl RenderObject for RenderCustomPaint {
///     fn layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Use maximum available space
///         constraints.biggest()
///     }
///
///     fn paint(&self, context: &mut PaintContext) {
///         self.painter.paint(context.canvas(), self.size);
///     }
/// }
/// ```
pub trait RenderObjectWidget: fmt::Debug + Send + Sync + 'static {
    /// The type of render object this widget creates
    ///
    /// This is the mutable object that performs layout and painting.
    type RenderObject: RenderObject;

    /// The arity (number of children) for this widget
    ///
    /// Must match the arity of the RenderObject.
    type Arity: crate::render::arity::Arity;

    /// Create a new render object
    ///
    /// Called once when the element is first created.
    /// The returned render object persists until the widget is removed.
    ///
    /// # Examples
    ///
    /// ```
    /// fn create_render_object(&self) -> Self::RenderObject {
    ///     RenderContainer {
    ///         width: self.width,
    ///         height: self.height,
    ///         color: self.color,
    ///     }
    /// }
    /// ```
    fn create_render_object(&self) -> Self::RenderObject;

    /// Update an existing render object
    ///
    /// Called when the widget configuration changes.
    /// Update only the properties that changed, and mark the render
    /// object for relayout/repaint if needed.
    ///
    /// # Performance Tips
    ///
    /// - Only update changed properties
    /// - Only mark dirty if actually needed
    /// - Use `mark_needs_layout()` if layout changed
    /// - Use `mark_needs_paint()` if only painting changed
    ///
    /// # Examples
    ///
    /// ```
    /// fn update_render_object(&self, render_object: &mut Self::RenderObject) {
    ///     let mut needs_layout = false;
    ///     let mut needs_paint = false;
    ///
    ///     if render_object.width != self.width {
    ///         render_object.width = self.width;
    ///         needs_layout = true;
    ///     }
    ///
    ///     if render_object.color != self.color {
    ///         render_object.color = self.color;
    ///         needs_paint = true;
    ///     }
    ///
    ///     if needs_layout {
    ///         render_object.mark_needs_layout();
    ///     } else if needs_paint {
    ///         render_object.mark_needs_paint();
    ///     }
    /// }
    /// ```
    fn update_render_object(&self, render_object: &mut Self::RenderObject);

    /// Called when render object is about to be removed
    ///
    /// Use this to clean up resources if needed.
    /// Default implementation does nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// fn did_unmount_render_object(&self, render_object: &mut Self::RenderObject) {
    ///     render_object.dispose_resources();
    /// }
    /// ```
    fn did_unmount_render_object(&self, _render_object: &mut Self::RenderObject) {
        // Default: do nothing
    }
}

/// Automatic Widget implementation for RenderObjectWidget
///
/// All RenderObjectWidget types automatically get Widget trait,
/// which in turn automatically get DynWidget via blanket impl.
///
/// # Element Type
///
/// RenderObjectWidget uses `RenderObjectElement<Self>` which:
/// - Creates and stores the RenderObject
/// - Attaches RenderObject to render tree
/// - Updates RenderObject when widget changes
/// - Manages RenderObject lifecycle
///
/// # State Type
///
/// Uses default `()` - render objects don't use the State system
/// (they have their own mutable state)
///
/// # Arity
///
/// Uses default `LeafArity` unless overridden.
/// Override for containers:
// Widget impl is now generated by #[derive(RenderObjectWidget)] macro
// This avoids blanket impl conflicts on stable Rust
//
// Use: #[derive(RenderObjectWidget)] on your widget type

// DynWidget comes automatically via blanket impl in mod.rs!

/// Helper trait to get child widget for single-child render widgets
///
/// Implement this on your RenderObjectWidget to provide the child widget.
pub trait SingleChildRenderObjectWidget: RenderObjectWidget {
    /// Get the child widget
    fn child(&self) -> &crate::BoxedWidget;
}

/// Helper trait to get children for multi-child render widgets
///
/// Implement this on your RenderObjectWidget to provide the children.
pub trait MultiChildRenderObjectWidget: RenderObjectWidget {
    /// Get the children widgets
    fn children(&self) -> &[crate::BoxedWidget];
}

// Note: Blanket impl for RenderObjectWidget -> Widget was removed
// because it conflicts with StatelessWidget -> Widget blanket impl.
//
// Instead, each RenderObjectWidget must manually implement Widget trait:
//
// impl Widget for MyRenderWidget {}
//
// Widget trait has default implementations for all methods, so the impl is trivial.
// Alternatively, use #[derive(RenderObjectWidget)] macro from flui_derive.

// Tests disabled - need to be updated for new API
#[cfg(all(test, disabled))]
mod tests {
    use super::*;
    use crate::Key;

    // Mock RenderObject for testing
    struct MockRenderObject {
        value: i32,
    }

    impl RenderObject for MockRenderObject {
        // Minimal implementation for testing
    }

    #[test]
    fn test_simple_render_object_widget() {
        #[derive(Debug)]
        struct TestWidget {
            value: i32,
        }

        impl RenderObjectWidget for TestWidget {
            type RenderObject = MockRenderObject;

            fn create_render_object(&self) -> Self::RenderObject {
                MockRenderObject { value: self.value }
            }

            fn update_render_object(&self, render_object: &mut Self::RenderObject) {
                render_object.value = self.value;
            }
        }

        let widget = TestWidget { value: 42 };

        // Create render object
        let render_object = widget.create_render_object();
        assert_eq!(render_object.value, 42);

        // Widget is automatic
        let _: &dyn Widget = &widget;

        // DynWidget is automatic
        let _: &dyn crate::DynWidget = &widget;
    }

    #[test]
    fn test_render_object_update() {
        #[derive(Debug)]
        struct TestWidget {
            value: i32,
        }

        impl RenderObjectWidget for TestWidget {
            type RenderObject = MockRenderObject;

            fn create_render_object(&self) -> Self::RenderObject {
                MockRenderObject { value: self.value }
            }

            fn update_render_object(&self, render_object: &mut Self::RenderObject) {
                render_object.value = self.value;
            }
        }

        let widget1 = TestWidget { value: 42 };
        let widget2 = TestWidget { value: 100 };

        let mut render_object = widget1.create_render_object();
        assert_eq!(render_object.value, 42);

        // Update with new widget
        widget2.update_render_object(&mut render_object);
        assert_eq!(render_object.value, 100);
    }

    #[test]
    fn test_render_object_widget_without_clone() {
        // RenderObjectWidget doesn't require Clone!
        #[derive(Debug)]
        struct NonCloneWidget {
            data: Vec<u8>,
        }

        impl RenderObjectWidget for NonCloneWidget {
            type RenderObject = MockRenderObject;

            fn create_render_object(&self) -> Self::RenderObject {
                MockRenderObject {
                    value: self.data.len() as i32,
                }
            }

            fn update_render_object(&self, render_object: &mut Self::RenderObject) {
                render_object.value = self.data.len() as i32;
            }
        }

        let widget = NonCloneWidget {
            data: vec![1, 2, 3],
        };

        // Can still box it
        let boxed: crate::BoxedWidget = Box::new(widget);
        assert!(boxed.is::<NonCloneWidget>());
    }

    #[test]
    fn test_single_child_render_widget() {
        #[derive(Debug)]
        struct PaddingWidget {
            child: crate::BoxedWidget,
        }

        impl RenderObjectWidget for PaddingWidget {
            type RenderObject = MockRenderObject;

            fn create_render_object(&self) -> Self::RenderObject {
                MockRenderObject { value: 0 }
            }

            fn update_render_object(&self, _render_object: &mut Self::RenderObject) {
                // Update logic
            }
        }

        impl Widget for PaddingWidget {
            // Element type and Arity determined by RenderObjectWidget impl
        }

        impl SingleChildRenderObjectWidget for PaddingWidget {
            fn child(&self) -> &crate::BoxedWidget {
                &self.child
            }
        }

        let widget = PaddingWidget {
            child: Box::new(MockWidget),
        };

        let _child = widget.child();
    }

    // Mock widget for testing
    #[derive(Debug)]
    struct MockWidget;

    impl Widget for MockWidget {
        // Element type determined by framework
    }

    impl crate::DynWidget for MockWidget {}

    #[derive(Debug)]
    struct MockElement;

    impl<W: Widget> crate::Element<W> for MockElement {
        fn new(_: W) -> Self {
            Self
        }
    }
}
