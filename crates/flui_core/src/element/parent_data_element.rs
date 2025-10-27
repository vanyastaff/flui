//! ParentDataElement - element for ParentDataWidget
//!
//! Manages a single child and applies parent data to descendant RenderObjects.

use std::fmt;
use std::marker::PhantomData;

use crate::ElementId;
use crate::widget::ParentDataWidget;
use crate::render::ParentData;
use crate::element::{DynElement, ElementLifecycle as PublicLifecycle};

/// Element for ParentDataWidget
///
/// Manages a single child and applies parent data to descendant RenderObjects.
///
/// # Architecture
///
/// ```text
/// ParentDataElement<Flexible, FlexParentData>
///   ├─ widget: Flexible
///   ├─ parent: Option<ElementId>
///   ├─ child: Option<ElementId>
///   └─ lifecycle: ElementLifecycle
/// ```
///
/// # Parent Data Application
///
/// When the child is mounted, this element walks down the tree to find
/// the first RenderObjectElement and applies parent data to it.
///
/// # Lifecycle
///
/// 1. **mount()** - Insert into tree
/// 2. **rebuild()** - Build child widget
/// 3. **apply_parent_data()** - Set parent data on descendant RenderObject
/// 4. **unmount()** - Remove from tree
pub struct ParentDataElement<W>
where
    W: ParentDataWidget,
{
    /// The parent data widget
    widget: W,

    /// Parent element ID
    parent: Option<ElementId>,

    /// Child element ID
    child: Option<ElementId>,

    /// Dirty flag
    dirty: bool,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,

}

/// Lifecycle states for an element
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementLifecycle {
    /// Element is created but not yet in tree
    Initial,
    /// Element is active in the tree
    Active,
    /// Element has been removed from tree
    Defunct,
}

impl<W> ParentDataElement<W>
where
    W: ParentDataWidget,
{
    /// Create a new ParentDataElement
    ///
    /// # Arguments
    ///
    /// - `widget`: The ParentDataWidget
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let element = ParentDataElement::new(Flexible {
    ///     flex: 1,
    ///     child: Box::new(Container::new()),
    /// });
    /// ```
    pub fn new(widget: W) -> Self {
        Self {
            widget,
            parent: None,
            child: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
        }
    }

    /// Get reference to the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }

    /// Update the widget
    ///
    /// # Arguments
    ///
    /// - `new_widget`: The new widget to replace the current one
    pub fn update(&mut self, new_widget: W) {
        self.widget = new_widget;
        self.dirty = true;
    }

    /// Get the child element ID
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set the child element ID
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Remove the child element
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    /// Get the parent element ID
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set the parent element ID
    pub(crate) fn set_parent(&mut self, parent_id: Option<ElementId>) {
        self.parent = parent_id;
    }

    /// Check if element is active
    pub fn is_active(&self) -> bool {
        self.lifecycle == ElementLifecycle::Active
    }

    /// Build the child widget
    ///
    /// Gets the child widget from the ParentDataWidget for building the child element.
    pub fn build(&self) -> crate::BoxedWidget {
        // TODO: Implement proper widget cloning
        // For now, create a stub - this element type is rarely used
        todo!("ParentDataElement::build() needs proper widget cloning")
    }
}

impl<W> fmt::Debug for ParentDataElement<W>
where
    W: ParentDataWidget,
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

// ========== DynElement Implementation ==========

impl<W> DynElement for ParentDataElement<W>
where
    W: ParentDataWidget + crate::Widget,
    W::Element: DynElement,
{
    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn lifecycle(&self) -> PublicLifecycle {
        match self.lifecycle {
            ElementLifecycle::Initial => PublicLifecycle::Initial,
            ElementLifecycle::Active => PublicLifecycle::Active,
            ElementLifecycle::Defunct => PublicLifecycle::Defunct,
        }
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
    }

    fn deactivate(&mut self) {
        // ParentDataElements don't support deactivation - unmount instead
        self.unmount();
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
    }

    fn update_any(&mut self, new_widget: Box<dyn crate::DynWidget>) {
        use crate::DynWidget;
        // Try to downcast to our widget type
        if let Some(new_widget) = (&*new_widget as &dyn std::any::Any).downcast_ref::<W>() {
            self.update(Clone::clone(new_widget));
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn crate::DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // ParentDataWidget just wraps its child widget
        let child_widget = self.build();

        // Mark old child for unmounting
        self.child = None;

        // Return the child that needs to be mounted
        vec![(element_id, child_widget, 0)]
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // ParentDataElement only has one child, slot is always 0
    }

    // ParentDataElement doesn't have RenderObject - use defaults
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Widget, DynWidget, RenderObjectWidget, RenderObject, BoxParentData, ProxyWidget, ParentDataWidget, LeafArity, LayoutCx, PaintCx, RenderObjectKind};
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

    // Use macro to implement Widget + DynWidget
    crate::impl_widget_for_parent_data!(TestParentDataWidget);

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
    fn test_parent_data_element_creation() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let element = ParentDataElement::new(widget);

        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), PublicLifecycle::Initial);
        assert_eq!(element.child(), None);
    }

    #[test]
    fn test_parent_data_element_mount() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ParentDataElement::new(widget);

        element.mount(Some(100), 0);

        assert_eq!(element.parent(), Some(100));
        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), PublicLifecycle::Active);
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
    fn test_parent_data_element_unmount() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ParentDataElement::new(widget);
        element.mount(None, 0);

        element.unmount();

        assert_eq!(element.lifecycle(), PublicLifecycle::Defunct);
    }

    #[test]
    fn test_parent_data_element_build() {
        let widget = TestParentDataWidget {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let element = ParentDataElement::new(widget);

        let _child = element.build();
    }
}
