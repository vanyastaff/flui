//! ParentDataWidget - Configures parent data on RenderObject children
//!
//! ParentDataWidget is used by layout widgets to attach layout-specific data
//! to their children in the element tree.

use super::{Widget, DynWidget, ProxyWidget, sealed};
use crate::render::ParentData;
use crate::element::ParentDataElement;

/// Widget that configures parent data on RenderObject children
///
/// ParentDataWidget is used by layout widgets to attach layout-specific data
/// to their children. For example:
/// - `Flexible` (for Row/Column) sets flex factor in FlexParentData
/// - `Positioned` (for Stack) sets offset in StackParentData
///
/// The parent data is created when the child is mounted.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{ParentDataWidget, ProxyWidget, ParentData};
/// use flui_rendering::FlexParentData;
///
/// #[derive(Debug, Clone)]
/// struct Flexible {
///     flex: i32,
///     child: Box<dyn DynWidget>,
/// }
///
/// impl ProxyWidget for Flexible {
///     fn child(&self) -> &dyn DynWidget {
///         &*self.child
///     }
/// }
///
/// impl ParentDataWidget<FlexParentData> for Flexible {
///     fn create_parent_data(&self) -> Box<dyn ParentData> {
///         Box::new(FlexParentData::new(self.flex, FlexFit::Loose))
///     }
///
///     fn debug_typical_ancestor_widget_class(&self) -> &'static str {
///         "Flex"
///     }
/// }
/// ```
///
/// # Automatic Widget Implementation
///
/// ParentDataWidget automatically implements `Widget` and `DynWidget` via blanket impl:
/// ```rust,ignore
/// impl<W, T> Widget for W where W: ParentDataWidget<T>, T: ParentData {
///     type Kind = ParentDataKind;
/// }
/// ```
pub trait ParentDataWidget<T: ParentData>: ProxyWidget {
    /// Create parent data for the child
    ///
    /// This is called when the child is mounted or when this widget updates.
    /// The returned ParentData will be stored in ElementTree for the child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn create_parent_data(&self) -> Box<dyn ParentData> {
    ///     Box::new(FlexParentData::new(self.flex, self.fit))
    /// }
    /// ```
    fn create_parent_data(&self) -> Box<dyn ParentData>;

    /// Debug: Typical ancestor widget class that should contain this widget
    ///
    /// For example, `Flexible` returns "Flex" (Row/Column).
    /// This is used for debug assertions and error messages.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn debug_typical_ancestor_widget_class(&self) -> &'static str {
    ///     "Flex"  // Flexible should be inside Row/Column
    /// }
    /// ```
    fn debug_typical_ancestor_widget_class(&self) -> &'static str;

    /// Can this widget apply parent data out of turn?
    ///
    /// Some parent data widgets can apply their data even if they're not
    /// direct children of the RenderObject widget. This is an optimization.
    ///
    /// Default is `false` - most parent data widgets must be direct children.
    fn debug_can_apply_out_of_turn(&self) -> bool {
        false
    }
}

// ========== Automatic Implementations ==========

/// Automatically implement sealed::Sealed for all ParentDataWidgets
///
/// This makes ParentDataWidget types eligible for the Widget trait.
/// The ElementType is set to ParentDataElement<W, T>.
impl<W, T> sealed::Sealed for W
where
    W: ParentDataWidget<T>,
    T: ParentData,
{
    type ElementType = ParentDataElement<W, T>;
}

/// Automatically implement Widget for all ParentDataWidgets
///
/// Thanks to the sealed trait pattern, this blanket impl doesn't conflict
/// with other widget type implementations.
impl<W, T> Widget for W
where
    W: ParentDataWidget<T>,
    T: ParentData,
{
    fn key(&self) -> Option<&str> {
        None
    }

    fn into_element(self) -> ParentDataElement<W, T> {
        ParentDataElement::new(self)
    }
}

/// Automatically implement DynWidget for all ParentDataWidgets
impl<W, T> DynWidget for W
where
    W: ParentDataWidget<T>,
    T: ParentData,
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxParentData, RenderObjectWidget, RenderObject, LeafArity, LayoutCx, PaintCx, RenderObjectKind};
    use flui_types::Size;
    use flui_engine::{BoxedLayer, ContainerLayer};

    // Test parent data widget
    #[derive(Debug)]
    struct TestParentDataWidget {
        value: i32,
        child: Box<dyn DynWidget>,
    }

    impl Clone for TestParentDataWidget {
        fn clone(&self) -> Self {
            Self {
                value: self.value,
                child: self.child.clone(),
            }
        }
    }

    impl ProxyWidget for TestParentDataWidget {
        fn child(&self) -> &dyn DynWidget {
            &*self.child
        }
    }

    impl ParentDataWidget<BoxParentData> for TestParentDataWidget {
        fn create_parent_data(&self) -> Box<dyn ParentData> {
            Box::new(BoxParentData::default())
        }

        fn debug_typical_ancestor_widget_class(&self) -> &'static str {
            "TestContainer"
        }
    }

    // Widget and DynWidget are automatically implemented via blanket impl!

    // Dummy child widget
    #[derive(Debug, Clone)]
    struct ChildWidget;

    impl Widget for ChildWidget {
        type Kind = RenderObjectKind;
    }

    impl DynWidget for ChildWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    impl RenderObjectWidget for ChildWidget {
        type Arity = LeafArity;
        type Render = ChildRender;

        fn create_render_object(&self) -> Self::Render {
            ChildRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct ChildRender;

    impl RenderObject for ChildRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::ZERO)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_parent_data_widget_creation() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };

        assert_eq!(widget.value, 42);
        let _child = widget.child();
    }

    #[test]
    fn test_parent_data_widget_create_parent_data() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };

        let parent_data = widget.create_parent_data();
        assert!(parent_data.is::<BoxParentData>());
    }

    #[test]
    fn test_parent_data_debug_typical_ancestor() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };

        assert_eq!(
            widget.debug_typical_ancestor_widget_class(),
            "TestContainer"
        );
    }

    #[test]
    fn test_parent_data_can_apply_out_of_turn() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };

        // Default implementation returns false
        assert!(!widget.debug_can_apply_out_of_turn());
    }

    #[test]
    fn test_parent_data_widget_clone() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };

        let cloned = widget.clone();
        assert_eq!(cloned.value, 42);
    }

    #[test]
    fn test_parent_data_widget_implements_widget() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };

        // Should compile - Widget is automatically implemented
        let _key: Option<&str> = widget.key();
    }
}
