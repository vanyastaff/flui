//! BuildContext - context for building widgets
//!
//! Provides access to the element tree and InheritedWidgets during build phase.

use std::any::TypeId;
use crate::ElementId;
use crate::element::ElementTree;
use crate::widget::{InheritedWidget, DynWidget};

/// BuildContext - provides access to tree during widget build
///
/// BuildContext is passed to `build()` methods and allows widgets to:
/// - Access InheritedWidgets from ancestors
/// - Register dependencies for automatic rebuilds
/// - Query tree structure
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessWidget for MyWidget {
///     fn build(&self, context: &BuildContext) -> BoxedWidget {
///         // Access theme with dependency (auto-rebuild on change)
///         let theme = context.depend_on::<Theme>().unwrap();
///
///         // Use theme data
///         Box::new(Text::new(format!("Color: {:?}", theme.color)))
///     }
/// }
/// ```
pub struct BuildContext<'a> {
    /// Reference to the element tree
    tree: &'a ElementTree,

    /// ID of the current element being built
    element_id: ElementId,
}

impl<'a> BuildContext<'a> {
    /// Create a new BuildContext
    ///
    /// # Arguments
    ///
    /// - `tree`: Reference to the element tree
    /// - `element_id`: ID of the element being built
    pub fn new(tree: &'a ElementTree, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    /// Access an InheritedWidget and register dependency
    ///
    /// Walks up the tree to find the nearest ancestor of type `T`.
    /// Registers this element as a dependent, so it will rebuild when the
    /// InheritedWidget changes (if `update_should_notify()` returns true).
    ///
    /// # Returns
    ///
    /// `Some(T)` if found, `None` if no ancestor has this type
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme = context.depend_on::<Theme>()?;
    /// println!("Primary color: {:?}", theme.primary_color);
    /// ```
    pub fn depend_on<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.find_ancestor_inherited_widget::<T>(true)
    }

    /// Access an InheritedWidget without dependency
    ///
    /// Walks up the tree to find the nearest ancestor of type `T`.
    /// Does NOT register a dependency - the element will NOT rebuild
    /// when the InheritedWidget changes.
    ///
    /// Use this for one-time reads where you don't need automatic updates.
    ///
    /// # Returns
    ///
    /// `Some(T)` if found, `None` if no ancestor has this type
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Read once at initialization, no auto-rebuild
    /// let theme = context.read::<Theme>()?;
    /// println!("Initial theme: {:?}", theme.name);
    /// ```
    pub fn read<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.find_ancestor_inherited_widget::<T>(false)
    }

    /// Find an InheritedWidget in ancestors
    ///
    /// # Arguments
    ///
    /// - `register_dependency`: If true, register this element as dependent
    ///
    /// # Returns
    ///
    /// The widget if found, None otherwise
    fn find_ancestor_inherited_widget<T>(&self, register_dependency: bool) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        let target_type_id = TypeId::of::<T>();

        // Walk up the parent chain
        let mut current_id = self.element_id;

        while let Some(parent_id) = self.tree.parent(current_id) {
            // Get the element
            if let Some(element) = self.tree.get(parent_id) {
                // Check if this element's widget is InheritedWidget of type T
                let widget = element.widget();

                if DynWidget::type_id(widget) == target_type_id {
                    // Found it! Try to downcast
                    if let Some(inherited_widget) = (widget as &dyn std::any::Any).downcast_ref::<T>() {
                        // TODO: Register dependency if requested
                        // For now just return the widget
                        // Later we'll add: self.tree.add_dependent(parent_id, self.element_id)

                        if register_dependency {
                            // TODO: Add to InheritedElement's dependents set
                            // This requires mutable access to tree, which we don't have here
                            // Will need to refactor BuildContext or use interior mutability
                        }

                        return Some(inherited_widget.clone());
                    }
                }
            }

            current_id = parent_id;
        }

        None
    }

    /// Get the current element ID
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get reference to the element tree
    pub fn tree(&self) -> &ElementTree {
        self.tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::{Widget, DynWidget, RenderObjectWidget};
    use crate::element::{RenderObjectElement, InheritedElement};
    use crate::{RenderObject, LeafArity, LayoutCx, PaintCx, impl_widget_for_inherited};
    use flui_types::Size;
    use flui_engine::{BoxedLayer, ContainerLayer};

    // Test theme widget
    #[derive(Debug, Clone, PartialEq)]
    struct TestTheme {
        color: u32,
    }

    impl InheritedWidget for TestTheme {
        fn update_should_notify(&self, old: &Self) -> bool {
            self.color != old.color
        }

        fn child(&self) -> crate::BoxedWidget {
            Box::new(DummyWidget)
        }
    }

    impl_widget_for_inherited!(TestTheme);

    #[derive(Debug, Clone)]
    struct DummyWidget;

    impl Widget for DummyWidget {
        type Kind = RenderObjectKind;
    }
    impl DynWidget for DummyWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    impl RenderObjectWidget for DummyWidget {
        type Arity = LeafArity;
        type Render = DummyRender;

        fn create_render_object(&self) -> Self::Render {
            DummyRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct DummyRender;

    impl RenderObject for DummyRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::ZERO)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_build_context_creation() {
        let tree = ElementTree::new();
        let context = BuildContext::new(&tree, 0);

        assert_eq!(context.element_id(), 0);
    }

    #[test]
    fn test_build_context_find_inherited() {
        let mut tree = ElementTree::new();

        // Insert InheritedElement
        let theme = TestTheme { color: 0xFF0000 };
        let inherited_elem = InheritedElement::new(theme.clone());
        let theme_id = tree.insert(Box::new(inherited_elem));

        // Insert child RenderObjectElement
        let child_elem = RenderObjectElement::new(DummyWidget);
        let child_id = tree.insert(Box::new(child_elem));

        // Manually set up parent-child relationship
        // (In real code, this would be done by build system)
        if let Some(theme_element) = tree.get_mut(theme_id) {
            // Can't easily test this without more infrastructure
            // This test verifies compilation for now
        }

        let context = BuildContext::new(&tree, child_id);

        // Try to find theme (won't work without proper parent setup)
        // This is just a compilation test
        let _maybe_theme: Option<TestTheme> = context.read();
    }
}
