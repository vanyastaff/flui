//! InheritedWidget access methods

use std::any::TypeId;
use crate::ElementId;
use crate::widget::InheritedWidget;
use super::Context;

impl Context {
    /// Access an InheritedWidget's data and establish dependency
    ///
    ///
    /// Note: W must implement Widget (use `impl_widget_for_inherited!` macro).
    pub fn depend_on_inherited_widget<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.get_inherited_widget_impl::<W>(TypeId::of::<W>(), true)
    }

    /// Access InheritedWidget - short form
    pub fn subscribe_to<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.depend_on_inherited_widget()
    }

    /// Access InheritedWidget without establishing dependency
    pub fn get_inherited_widget<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.get_inherited_widget_impl::<W>(TypeId::of::<W>(), false)
    }

    /// Access InheritedWidget without dependency - short form
    pub fn find_inherited<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.get_inherited_widget()
    }

    /// Internal implementation for getting inherited widgets
    fn get_inherited_widget_impl<W>(
        &self,
        _type_id: TypeId,
        register_dependency: bool,
    ) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        use crate::widget::InheritedElement;

        let tree = self.tree();
        let mut current_id = self.parent();

        // Walk up the tree looking for InheritedElement<W>
        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                // Try to downcast to InheritedElement<W>
                if let Some(inherited_elem) = element.downcast_ref::<InheritedElement<W>>() {
                    // Found matching InheritedWidget!

                    if register_dependency {
                        // Drop read lock before acquiring write lock
                        drop(tree);

                        // Register dependency
                        let mut tree_mut = self.tree.write();
                        if let Some(inherited_elem_mut) = tree_mut
                            .get_mut(id)
                            .and_then(|e| e.downcast_mut::<InheritedElement<W>>())
                        {
                            inherited_elem_mut.register_dependent(self.element_id);
                        }

                        // Re-acquire read lock to get widget
                        let tree = self.tree.read();
                        if let Some(inherited_elem) = tree
                            .get(id)
                            .and_then(|e| e.downcast_ref::<InheritedElement<W>>())
                        {
                            return Some(inherited_elem.widget().clone());
                        }
                        return None;
                    } else {
                        // No dependency registration
                        return Some(inherited_elem.widget().clone());
                    }
                }

                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }

    /// Find the element for an inherited widget
    ///
    /// Low-level API for advanced use cases.
    pub fn get_element_for_inherited_widget_of_exact_type<W>(
        &self,
    ) -> Option<ElementId>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        use crate::widget::InheritedElement;

        let tree = self.tree.read();
        let mut current_id = self.parent();

        while let Some(id) = current_id {
            if let Some(element) = tree.get(id) {
                if element.is::<InheritedElement<W>>() {
                    return Some(id);
                }
                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }

    /// Find inherited element - short form
    pub fn find_inherited_element<W>(
        &self,
    ) -> Option<ElementId>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.get_element_for_inherited_widget_of_exact_type::<W>()
    }
}
