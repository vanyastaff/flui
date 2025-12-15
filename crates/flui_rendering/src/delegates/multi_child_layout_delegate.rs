//! Multi-child layout delegate for custom layout algorithms with multiple children.
//!
//! [`MultiChildLayoutDelegate`] allows users to implement custom layout
//! behavior for render objects with multiple children identified by IDs.

use std::any::Any;
use std::fmt::Debug;

use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;

/// A delegate that provides custom layout behavior for multiple children.
///
/// Unlike single-child layout, multi-child layout requires identifying
/// children by ID strings. This allows the delegate to lay out children
/// in a specific order and position them relative to each other.
///
/// # Layout Context
///
/// The delegate works with a [`MultiChildLayoutContext`] that provides
/// methods to layout and position children. The context is provided
/// during the `perform_layout` call.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::{MultiChildLayoutDelegate, MultiChildLayoutContext};
/// use flui_types::{BoxConstraints, Offset, Size};
///
/// #[derive(Debug)]
/// struct DialogLayoutDelegate {
///     padding: f32,
/// }
///
/// impl MultiChildLayoutDelegate for DialogLayoutDelegate {
///     fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
///         let inner_width = size.width - 2.0 * self.padding;
///         let mut y = self.padding;
///
///         // Layout title
///         if context.has_child("title") {
///             let title_constraints = BoxConstraints::tight_for(Some(inner_width), None);
///             let title_size = context.layout_child("title", title_constraints);
///             context.position_child("title", Offset::new(self.padding, y));
///             y += title_size.height + self.padding;
///         }
///
///         // Layout content
///         if context.has_child("content") {
///             let content_constraints = BoxConstraints::tight_for(Some(inner_width), None);
///             let content_size = context.layout_child("content", content_constraints);
///             context.position_child("content", Offset::new(self.padding, y));
///         }
///     }
///
///     fn get_size(&self, constraints: BoxConstraints) -> Size {
///         constraints.biggest()
///     }
///
///     fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.padding != old.padding
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait MultiChildLayoutDelegate: Send + Sync + Debug {
    /// Perform layout of children.
    ///
    /// Use the context to query, layout, and position children by their IDs.
    /// Children must be laid out before they can be positioned.
    ///
    /// # Arguments
    ///
    /// * `context` - The layout context providing child operations
    /// * `size` - The size of this render object
    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size);

    /// Get the size of the parent for the given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The size of this render object.
    fn get_size(&self, constraints: BoxConstraints) -> Size;

    /// Whether to relayout when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous layout delegate
    ///
    /// # Returns
    ///
    /// `true` if layout should be recalculated, `false` otherwise.
    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool;

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Context for multi-child layout operations.
///
/// This trait is implemented by the render object and passed to the delegate
/// during layout. It provides methods to query, layout, and position children.
pub trait MultiChildLayoutContext {
    /// Check if a child with the given ID exists.
    fn has_child(&self, child_id: &str) -> bool;

    /// Layout a child with the given constraints and return its size.
    ///
    /// # Panics
    ///
    /// Panics if the child doesn't exist or has already been laid out.
    fn layout_child(&mut self, child_id: &str, constraints: BoxConstraints) -> Size;

    /// Position a child at the given offset.
    ///
    /// # Panics
    ///
    /// Panics if the child hasn't been laid out yet.
    fn position_child(&mut self, child_id: &str, offset: Offset);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Debug)]
    struct TestDelegate {
        padding: f32,
    }

    impl MultiChildLayoutDelegate for TestDelegate {
        fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
            if context.has_child("header") {
                let constraints = BoxConstraints::tight_for(Some(size.width), None);
                let header_size = context.layout_child("header", constraints);
                context.position_child("header", Offset::new(0.0, 0.0));

                if context.has_child("body") {
                    let body_constraints = BoxConstraints::tight_for(
                        Some(size.width),
                        Some(size.height - header_size.height - self.padding),
                    );
                    context.layout_child("body", body_constraints);
                    context.position_child(
                        "body",
                        Offset::new(0.0, header_size.height + self.padding),
                    );
                }
            }
        }

        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
            if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
                (self.padding - old.padding).abs() > f32::EPSILON
            } else {
                true
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct MockContext {
        children: HashMap<String, Size>,
        laid_out: HashMap<String, Size>,
        positions: HashMap<String, Offset>,
    }

    impl MockContext {
        fn new() -> Self {
            let mut children = HashMap::new();
            children.insert("header".to_string(), Size::new(100.0, 50.0));
            children.insert("body".to_string(), Size::new(100.0, 200.0));

            Self {
                children,
                laid_out: HashMap::new(),
                positions: HashMap::new(),
            }
        }
    }

    impl MultiChildLayoutContext for MockContext {
        fn has_child(&self, child_id: &str) -> bool {
            self.children.contains_key(child_id)
        }

        fn layout_child(&mut self, child_id: &str, _constraints: BoxConstraints) -> Size {
            let size = self.children.get(child_id).copied().unwrap();
            self.laid_out.insert(child_id.to_string(), size);
            size
        }

        fn position_child(&mut self, child_id: &str, offset: Offset) {
            self.positions.insert(child_id.to_string(), offset);
        }
    }

    #[test]
    fn test_multi_child_layout() {
        let delegate = TestDelegate { padding: 10.0 };
        let mut context = MockContext::new();
        let size = Size::new(200.0, 300.0);

        delegate.perform_layout(&mut context, size);

        assert!(context.laid_out.contains_key("header"));
        assert!(context.laid_out.contains_key("body"));

        let header_pos = context.positions.get("header").unwrap();
        assert_eq!(header_pos.dx, 0.0);
        assert_eq!(header_pos.dy, 0.0);

        let body_pos = context.positions.get("body").unwrap();
        assert_eq!(body_pos.dx, 0.0);
        assert_eq!(body_pos.dy, 60.0); // 50 (header height) + 10 (padding)
    }

    #[test]
    fn test_should_relayout() {
        let delegate1 = TestDelegate { padding: 10.0 };
        let delegate2 = TestDelegate { padding: 10.0 };
        let delegate3 = TestDelegate { padding: 20.0 };

        assert!(!delegate1.should_relayout(&delegate2));
        assert!(delegate1.should_relayout(&delegate3));
    }
}
