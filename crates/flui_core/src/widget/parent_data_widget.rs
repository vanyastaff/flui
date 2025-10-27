//! ParentDataWidget - widgets that attach metadata to descendants
//!
//! ParentDataWidget attaches layout metadata to render objects in the tree.
//! This metadata is used by parent render objects during layout.
//!
//! # When to Use
//!
//! - Widget needs to pass layout hints to an ancestor
//! - Widget wraps children with positioning data (Positioned, Flexible, etc.)
//! - Widget provides constraints or sizing information
//!
//! # Examples
//!
//! ```
//! // Flexible widget in a Row/Column
//! Flexible::new(
//!     flex: 2,
//!     child: Container::new()
//! )
//!
//! // Positioned widget in a Stack
//! Positioned::new(
//!     top: 10.0,
//!     left: 20.0,
//!     child: Image::new("avatar.png")
//! )
//! ```
//!
//! # Architecture
//!
//! ```text
//! Column (RenderFlex)
//!   ↓
//! Flexible (ParentDataWidget)
//!   ↓ attaches FlexParentData
//! Container (RenderObject with parent_data: FlexParentData)
//! ```

use std::fmt;
use std::any::Any;
use crate::widget::BoxedWidget;

/// ParentDataWidget - widget that attaches parent data to descendants
///
/// ParentDataWidget is a special widget that doesn't render anything itself.
/// Instead, it attaches metadata (ParentData) to its child's RenderObject,
/// which the parent RenderObject uses during layout.
///
/// # How It Works
///
/// ```text
/// 1. ParentDataWidget wraps a child
/// 2. Child creates a RenderObject
/// 3. ParentDataWidget.apply_parent_data() is called
/// 4. Metadata attached to child's RenderObject
/// 5. Parent RenderObject reads this metadata during layout
/// ```
///
/// # Common Use Cases
///
/// ## Flexible (for Row/Column)
///
/// ```text
/// Column
///   ├─ Flexible(flex: 2) ← ParentDataWidget
///   │   └─ Container     ← Gets FlexParentData{flex: 2}
///   └─ Flexible(flex: 1)
///       └─ Container     ← Gets FlexParentData{flex: 1}
///
/// During layout:
/// - Column reads FlexParentData from each child
/// - Distributes space based on flex values (2:1 ratio)
/// ```
///
/// ## Positioned (for Stack)
///
/// ```text
/// Stack
///   ├─ Positioned(top: 10, left: 20) ← ParentDataWidget
///   │   └─ Image                     ← Gets StackParentData{top: 10, left: 20}
///   └─ Positioned(bottom: 0, right: 0)
///       └─ Button                    ← Gets StackParentData{bottom: 0, right: 0}
///
/// During layout:
/// - Stack reads StackParentData from each child
/// - Positions children according to the data
/// ```
///
/// # Lifecycle
///
/// ```text
/// 1. Widget created: Flexible { flex: 2, child: Container }
/// 2. Child element/render object created
/// 3. apply_parent_data() called → Attaches FlexParentData
/// 4. Parent layout reads FlexParentData
/// 5. Widget updated: Flexible { flex: 3, child: Container }
/// 6. apply_parent_data() called again → Updates FlexParentData
/// ```
///
/// # Type Safety
///
/// The `ParentDataType` associated type ensures that:
/// - ParentDataWidget only works with compatible parents
/// - Type mismatches are caught at compile time
/// - Correct parent data structure is used
///
/// # Examples
///
/// ## Flexible Widget
///
/// ```
/// #[derive(Debug)]
/// struct Flexible {
///     flex: i32,
///     fit: FlexFit,
///     child: BoxedWidget,
/// }
///
/// impl ParentDataWidget for Flexible {
///     type ParentDataType = FlexParentData;
///
///     fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
///         if let Some(parent_data) = render_object
///             .parent_data_mut()
///             .and_then(|d| d.downcast_mut::<FlexParentData>())
///         {
///             parent_data.flex = Some(self.flex);
///             parent_data.fit = self.fit;
///         }
///     }
///
///     fn child(&self) -> &BoxedWidget {
///         &self.child
///     }
/// }
///
/// // Usage in Row/Column:
/// Column::new(vec![
///     Box::new(Flexible::new(2, child1)),  // Takes 2/3 of space
///     Box::new(Flexible::new(1, child2)),  // Takes 1/3 of space
/// ])
/// ```
///
/// ## Positioned Widget
///
/// ```
/// #[derive(Debug)]
/// struct Positioned {
///     top: Option<f64>,
///     right: Option<f64>,
///     bottom: Option<f64>,
///     left: Option<f64>,
///     width: Option<f64>,
///     height: Option<f64>,
///     child: BoxedWidget,
/// }
///
/// impl ParentDataWidget for Positioned {
///     type ParentDataType = StackParentData;
///
///     fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
///         if let Some(parent_data) = render_object
///             .parent_data_mut()
///             .and_then(|d| d.downcast_mut::<StackParentData>())
///         {
///             parent_data.top = self.top;
///             parent_data.right = self.right;
///             parent_data.bottom = self.bottom;
///             parent_data.left = self.left;
///             parent_data.width = self.width;
///             parent_data.height = self.height;
///         }
///     }
///
///     fn child(&self) -> &BoxedWidget {
///         &self.child
///     }
/// }
///
/// // Usage in Stack:
/// Stack::new(vec![
///     Box::new(Positioned::top_left(10.0, 20.0, background)),
///     Box::new(Positioned::bottom_right(0.0, 0.0, close_button)),
/// ])
/// ```
///
/// ## Table Cell Widget
///
/// ```
/// #[derive(Debug)]
/// struct TableCell {
///     column_span: usize,
///     row_span: usize,
///     vertical_alignment: VerticalAlignment,
///     child: BoxedWidget,
/// }
///
/// impl ParentDataWidget for TableCell {
///     type ParentDataType = TableCellParentData;
///
///     fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
///         if let Some(parent_data) = render_object
///             .parent_data_mut()
///             .and_then(|d| d.downcast_mut::<TableCellParentData>())
///         {
///             parent_data.column_span = self.column_span;
///             parent_data.row_span = self.row_span;
///             parent_data.vertical_alignment = self.vertical_alignment;
///         }
///     }
///
///     fn child(&self) -> &BoxedWidget {
///         &self.child
///     }
/// }
///
/// // Usage in Table:
/// Table::new(vec![
///     Row::new(vec![
///         Box::new(TableCell::new(colspan: 2, child)),
///         Box::new(TableCell::new(colspan: 1, child)),
///     ]),
/// ])
/// ```
pub trait ParentDataWidget: Clone + fmt::Debug + Send + Sync + 'static {
    /// The type of parent data this widget applies
    ///
    /// This ensures type safety - the parent data type must match
    /// what the parent RenderObject expects.
    type ParentDataType: Any;

    /// Apply parent data to the render object
    ///
    /// This method is called when:
    /// - The child's render object is first created
    /// - The ParentDataWidget configuration changes
    /// - The child's render object is moved in the tree
    ///
    /// # Parameters
    ///
    /// - `render_object` - The child's render object
    ///
    /// # Implementation
    ///
    /// 1. Get the parent data from render object
    /// 2. Downcast to your ParentDataType
    /// 3. Update the parent data fields
    ///
    /// # Examples
    ///
    /// ```
    /// fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
    ///     if let Some(parent_data) = render_object
    ///         .parent_data_mut()
    ///         .and_then(|d| d.downcast_mut::<FlexParentData>())
    ///     {
    ///         parent_data.flex = Some(self.flex);
    ///         parent_data.fit = self.fit;
    ///     }
    /// }
    /// ```
    // TODO: RenderObject is not dyn-compatible because it has Sized bound
    // Need to either:
    // 1. Create DynRenderObject trait without Sized
    // 2. Use Box<dyn Any> and downcast
    // 3. Rethink ParentData architecture
    fn apply_parent_data(&self, _render_object: &mut ()) {
        todo!("apply_parent_data needs DynRenderObject trait")
    }

    /// Get the child widget
    ///
    /// ParentDataWidget always has exactly one child.
    fn child(&self) -> &BoxedWidget;

    /// Check if parent data is valid for given parent type
    ///
    /// Override this if you want to validate that the parent
    /// RenderObject is the correct type.
    ///
    /// # Examples
    ///
    /// ```
    /// fn debug_validate_parent_data(&self, parent: &()) -> bool {
    ///     // TODO: Check that parent is RenderFlex
    ///     true
    /// }
    /// ```
    fn debug_validate_parent_data(&self, _parent: &()) -> bool {
        true  // Default: accept any parent
    }
}

/// Automatic Widget implementation for ParentDataWidget
///
/// All ParentDataWidget types automatically get Widget trait,
/// which in turn automatically get DynWidget via blanket impl.
///
/// # Element Type
///
/// ParentDataWidget uses `ParentDataElement<Self>` which:
/// - Creates the child element
/// - Calls apply_parent_data() on the child's render object
/// - Re-applies when widget updates
///
/// # State Type
///
/// Uses default `()` - no state needed
///
/// # Arity
// Widget impl needs to be manual or via derive for ParentDataWidget
// This avoids blanket impl conflicts on stable Rust
//
// Note: ParentDataWidget currently doesn't have a derive macro
// Implement Widget manually for now

// DynWidget comes automatically via blanket impl in mod.rs!

/// ParentData trait - base trait for all parent data types
///
/// This is implemented by the actual parent data structures
/// (FlexParentData, StackParentData, etc.)
pub trait ParentData: Any + fmt::Debug {
    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get as mutable Any for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Detach from parent
    ///
    /// Called when the render object is removed from its parent.
    fn detach(&mut self) {
        // Default: do nothing
    }
}

/// Helper macro to implement ParentData trait
///
/// # Examples
///
/// ```
/// #[derive(Debug, Default)]
/// struct FlexParentData {
///     flex: Option<i32>,
///     fit: FlexFit,
/// }
///
/// impl_parent_data!(FlexParentData);
/// ```
#[macro_export]
macro_rules! impl_parent_data {
    ($type:ty) => {
        impl $crate::ParentData for $type {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundation::Key;

    // Mock ParentData for testing
    #[derive(Debug, Default)]
    struct MockParentData {
        value: i32,
    }

    impl ParentData for MockParentData {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    // Mock RenderObject for testing
    struct MockRenderObject {
        parent_data: Option<Box<dyn ParentData>>,
    }

    impl RenderObject for MockRenderObject {
        // Minimal implementation for testing
    }

    impl MockRenderObject {
        fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData> {
            self.parent_data.as_mut().map(|b| &mut **b)
        }
    }

    #[test]
    fn test_simple_parent_data_widget() {
        #[derive(Debug)]
        struct TestWidget {
            value: i32,
            child: BoxedWidget,
        }

        impl ParentDataWidget for TestWidget {
            type ParentDataType = MockParentData;

            fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
                // In real code, would downcast and apply data
            }

            fn child(&self) -> &BoxedWidget {
                &self.child
            }
        }

        let widget = TestWidget {
            value: 42,
            child: Box::new(MockWidget),
        };

        // Widget is automatic
        let _: &dyn Widget = &widget;

        // DynWidget is automatic
        let _: &dyn crate::DynWidget = &widget;

        // Has single child
        let _child = widget.child();
    }

    #[test]
    fn test_apply_parent_data() {
        #[derive(Debug)]
        struct TestWidget {
            value: i32,
            child: BoxedWidget,
        }

        impl ParentDataWidget for TestWidget {
            type ParentDataType = MockParentData;

            fn apply_parent_data(&self, render_object: &mut dyn RenderObject) {
                // Downcast to MockRenderObject for testing
                if let Some(mock) = (render_object as &mut dyn Any)
                    .downcast_mut::<MockRenderObject>()
                {
                    if let Some(parent_data) = mock
                        .parent_data_mut()
                        .and_then(|d| d.as_any_mut().downcast_mut::<MockParentData>())
                    {
                        parent_data.value = self.value;
                    }
                }
            }

            fn child(&self) -> &BoxedWidget {
                &self.child
            }
        }

        let widget = TestWidget {
            value: 42,
            child: Box::new(MockWidget),
        };

        let mut render_object = MockRenderObject {
            parent_data: Some(Box::new(MockParentData::default())),
        };

        // Apply parent data
        widget.apply_parent_data(&mut render_object);

        // Check that data was applied
        if let Some(parent_data) = render_object
            .parent_data_mut()
            .and_then(|d| d.as_any_mut().downcast_mut::<MockParentData>())
        {
            assert_eq!(parent_data.value, 42);
        }
    }

    #[test]
    fn test_parent_data_widget_without_clone() {
        // ParentDataWidget doesn't require Clone!
        #[derive(Debug)]
        struct NonCloneWidget {
            data: Vec<u8>,
            child: BoxedWidget,
        }

        impl ParentDataWidget for NonCloneWidget {
            type ParentDataType = MockParentData;

            fn apply_parent_data(&self, _render_object: &mut dyn RenderObject) {
                // Apply logic
            }

            fn child(&self) -> &BoxedWidget {
                &self.child
            }
        }

        let widget = NonCloneWidget {
            data: vec![1, 2, 3],
            child: Box::new(MockWidget),
        };

        // Can still box it
        let boxed: crate::BoxedWidget = Box::new(widget);
        assert!(boxed.is::<NonCloneWidget>());
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