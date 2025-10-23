//! ParentDataWidget - Configures parent data on RenderObject children

use std::any::TypeId;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::{
    DynElement, DynRenderObject, DynWidget, Element, ElementId, ElementLifecycle, ElementTree,
    ParentData, ProxyWidget,
};

/// Widget that configures parent data on RenderObject children
///
/// ParentDataWidget is used by layout widgets to attach layout-specific data
/// to their children in the ElementTree. For example:
/// - `Positioned` (for Stack) sets offset in StackParentData
/// - `Flexible` (for Row/Column) sets flex factor in FlexParentData
///
/// The parent data is created when the child is mounted.
pub trait ParentDataWidget<T: ParentData>: ProxyWidget {
    /// Create parent data for the child
    ///
    /// This is called when the child is mounted or when this widget updates.
    /// The returned ParentData will be stored in ElementTree for the child.
    fn create_parent_data(&self) -> Box<dyn ParentData>;

    /// Debug: Typical ancestor widget class that should contain this widget
    ///
    /// For example, `Flexible` returns "Flex" (Row/Column)
    fn debug_typical_ancestor_widget_class(&self) -> &'static str;

    /// Can this widget apply parent data out of turn?
    ///
    /// Some parent data widgets can apply their data even if they're not
    /// direct children of the RenderObject widget. This is an optimization.
    fn debug_can_apply_out_of_turn(&self) -> bool {
        false
    }
}

/// Element for ParentDataWidget
///
/// Manages a single child and applies parent data to descendant RenderObjects.
pub struct ParentDataElement<W, T>
where
    W: ParentDataWidget<T>,
    T: ParentData,
{    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    tree: Option<Arc<RwLock<ElementTree>>>,
    child: Option<ElementId>,
    _phantom: PhantomData<T>,
}

impl<W, T> ParentDataElement<W, T>
where
    W: ParentDataWidget<T>,
    T: ParentData,
{
    pub fn new(widget: W) -> Self {
        Self {            widget,
            parent: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
            tree: None,
            child: None,
            _phantom: PhantomData,
        }
    }

    /// Apply parent data to all descendant RenderObjects
    ///
    /// This walks down the tree and sets parent data on the first
    /// RenderObject element it finds.
    fn apply_parent_data_to_descendants(&self) {
        if let Some(child_id) = self.child {
            if let Some(tree) = &self.tree {
                self.set_parent_data_on_render_object(tree, child_id);
            }
        }
    }

    /// Recursively find RenderObject element and set parent data
    fn set_parent_data_on_render_object(&self, tree: &Arc<RwLock<ElementTree>>, element_id: ElementId) {
        let tree_guard = tree.read();

        if let Some(element) = tree_guard.get(element_id) {
            // If this element has a RenderObject, set parent data in tree
            if element.render_object().is_some() {
                drop(tree_guard);

                // Create parent data and set it in ElementTree
                let parent_data = self.widget.create_parent_data();
                tree.write().set_parent_data(element_id, parent_data);

                tracing::debug!("ParentDataElement: set parent_data for RenderObject element {}", element_id);
            } else {
                // No RenderObject, recurse into children to find one
                let children: Vec<ElementId> = element.children_iter().collect();
                drop(tree_guard);

                for child_id in children {
                    self.set_parent_data_on_render_object(tree, child_id);
                }
            }
        }
    }

    /// Get the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }
}

impl<W, T> fmt::Debug for ParentDataElement<W, T>
where
    W: ParentDataWidget<T>,
    T: ParentData,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParentDataElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("lifecycle", &self.lifecycle)
            .field("child", &self.child)
            .finish()
    }
}

// ========== Implement DynElement for ParentDataElement ==========

impl<W, T> DynElement for ParentDataElement<W, T>
where
    W: ParentDataWidget<T> + crate::Widget<Element = ParentDataElement<W, T>>,
    T: ParentData,
{    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn crate::foundation::Key> {
        ProxyWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().remove(child_id);
            }
        }

        self.lifecycle = ElementLifecycle::Defunct;
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        // Try to downcast to our widget type
        if let Ok(new_widget_typed) = new_widget.downcast::<W>() {
            self.widget = *new_widget_typed;
            self.mark_dirty();

            // Re-apply parent data after update
            self.apply_parent_data_to_descendants();
        }
    }

    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // ParentDataWidget just wraps its child widget
        let child_widget: Box<dyn DynWidget> = dyn_clone::clone_box(self.widget.child());

        // Mark old child for unmounting
        self.child = None;

        // Return the child that needs to be mounted
        vec![(element_id, child_widget, 0)]
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.child.take()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.child = Some(child_id);

        // Apply parent data after child is mounted
        self.apply_parent_data_to_descendants();
    }

    fn widget_type_id(&self) -> TypeId {
        TypeId::of::<W>()
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
    }

    fn render_object(&self) -> Option<&dyn DynRenderObject> {
        None // ParentDataElement doesn't have RenderObject
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn DynRenderObject> {
        None // ParentDataElement doesn't have RenderObject
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Default: do nothing (single child)
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }
}

// ========== Implement Element for ParentDataElement ==========

impl<W, T> Element for ParentDataElement<W, T>
where
    W: ParentDataWidget<T> + crate::Widget<Element = ParentDataElement<W, T>>,
    T: ParentData,
{
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        self.widget = new_widget;
        self.mark_dirty();

        // Re-apply parent data after update
        self.apply_parent_data_to_descendants();
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}

/// Macro to implement Widget for ParentDataWidget types
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct Flexible {
///     flex: u32,
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
///     fn apply_parent_data(&self, render_object: &mut dyn DynRenderObject) {
///         // Apply flex data
///     }
///
///     fn debug_typical_ancestor_widget_class(&self) -> &'static str {
///         "Flex"
///     }
/// }
///
/// impl_widget_for_parent_data!(Flexible, FlexParentData);
/// ```
#[macro_export]
macro_rules! impl_widget_for_parent_data {
    ($widget_type:ty, $parent_data_type:ty) => {
        impl $crate::Widget for $widget_type {
            type Element = $crate::ParentDataElement<$widget_type, $parent_data_type>;

            fn into_element(self) -> Self::Element {
                $crate::ParentDataElement::new(self)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxParentData, Context, StatelessWidget};

    // Test parent data widget
    #[derive(Debug, Clone)]
    struct TestParentDataWidget {
        value: i32,
        child: Box<dyn DynWidget>,
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

    impl_widget_for_parent_data!(TestParentDataWidget, BoxParentData);

    // Dummy child widget
    #[derive(Debug, Clone)]
    struct ChildWidget;

    impl StatelessWidget for ChildWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(ChildWidget)
        }
    }

    #[test]
    fn test_parent_data_widget_create_element() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let element = widget.create_element();

        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_parent_data_element_mount() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ParentDataElement::new(widget);

        let parent_id = unsafe { ElementId::from_raw(100) };
        element.mount(Some(parent_id), 0);

        assert_eq!(element.parent(), Some(parent_id));
        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_parent_data_element_update() {
        let widget1 = TestParentDataWidget {
            value: 1,
            child: Box::new(ChildWidget),
        };
        let mut element = ParentDataElement::new(widget1);

        let widget2 = TestParentDataWidget {
            value: 2,
            child: Box::new(ChildWidget),
        };
        element.update(widget2);

        assert_eq!(element.widget().value, 2);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_parent_data_debug_typical_ancestor() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let element = ParentDataElement::new(widget);

        assert_eq!(
            element.widget().debug_typical_ancestor_widget_class(),
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
}
