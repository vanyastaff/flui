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

    // ========== Phase 6: Enhanced Flutter-style API ==========

    /// Create a dependency on an InheritedElement (Phase 6)
    ///
    /// Low-level method that uses AnyElement trait methods.
    /// Prefer using typed methods like `depend_on_inherited_widget_of_exact_type<T>()`.
    ///
    /// # Parameters
    ///
    /// - `ancestor_id`: ID of the InheritedElement to depend on
    /// - `aspect`: Optional aspect for partial dependencies (future: InheritedModel)
    pub fn depend_on_inherited_element(
        &self,
        ancestor_id: ElementId,
        aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) {
        let mut tree = self.tree.write();
        if let Some(ancestor) = tree.get_mut(ancestor_id) {
            ancestor.register_dependency(self.element_id, aspect);
        }
    }

    /// Get and depend on an InheritedWidget of exact type T (Phase 6)
    ///
    /// This creates a dependency, so the current element will rebuild
    /// when the InheritedWidget changes.
    ///
    /// Returns a cloned widget if found, None otherwise.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The InheritedWidget type to find
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In build() method:
    /// if let Some(theme) = context.depend_on_inherited_widget_of_exact_type::<Theme>() {
    ///     println!("Theme color: {:?}", theme.color);
    /// }
    /// ```
    pub fn depend_on_inherited_widget_of_exact_type<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        // Find the InheritedWidget ancestor
        let ancestor_id = self.find_ancestor_inherited_element_of_type::<T>()?;

        // Register dependency
        self.depend_on_inherited_element(ancestor_id, None);

        // Return cloned widget
        let tree = self.tree.read();
        if let Some(element) = tree.get(ancestor_id) {
            if let Some(widget_any) = element.widget_as_any() {
                if let Some(widget) = widget_any.downcast_ref::<T>() {
                    return Some(widget.clone());
                }
            }
        }

        None
    }

    /// Get InheritedWidget without creating dependency (Phase 6)
    ///
    /// This does NOT cause rebuilds when the widget changes.
    /// Use this when you only need to read the value once.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Read theme once without dependency
    /// if let Some(theme) = context.get_inherited_widget_of_exact_type::<Theme>() {
    ///     println!("Theme initialized with color: {:?}", theme.color);
    /// }
    /// ```
    pub fn get_inherited_widget_of_exact_type<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        let ancestor_id = self.find_ancestor_inherited_element_of_type::<T>()?;

        let tree = self.tree.read();
        if let Some(element) = tree.get(ancestor_id) {
            if let Some(widget_any) = element.widget_as_any() {
                if let Some(widget) = widget_any.downcast_ref::<T>() {
                    return Some(widget.clone());
                }
            }
        }

        None
    }

    /// Helper: Find ancestor InheritedElement of type T (Phase 6)
    ///
    /// Returns the ElementId of the first ancestor InheritedElement
    /// whose widget has type T.
    fn find_ancestor_inherited_element_of_type<T>(&self) -> Option<ElementId>
    where
        T: InheritedWidget + 'static,
    {
        let tree = self.tree.read();
        let target_type_id = TypeId::of::<T>();

        let mut current = self.parent();
        while let Some(parent_id) = current {
            if let Some(element) = tree.get(parent_id) {
                // Check if this is an InheritedElement with widget type T
                if element.widget_has_type_id(target_type_id) {
                    return Some(parent_id);
                }
                current = element.parent();
            } else {
                break;
            }
        }

        None
    }

    // ========== Rust-Idiomatic Short Names (Phase 6 Ergonomics) ==========

    /// Get InheritedWidget and create dependency (Rust-idiomatic short name)
    ///
    /// Short, ergonomic alternative to `depend_on_inherited_widget_of_exact_type<T>()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Short and sweet! 🎉
    /// let theme = context.inherit::<Theme>();
    /// ```
    pub fn inherit<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.depend_on_inherited_widget_of_exact_type::<T>()
    }

    /// Get InheritedWidget without dependency (Rust-idiomatic short name)
    ///
    /// Short, ergonomic alternative to `get_inherited_widget_of_exact_type<T>()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Short read without dependency
    /// let theme = context.read_inherited::<Theme>();
    /// ```
    pub fn read_inherited<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.get_inherited_widget_of_exact_type::<T>()
    }

    /// Get InheritedWidget and create dependency (short alias)
    ///
    /// Alternative name for `inherit()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme = context.watch::<Theme>();  // Like React hooks!
    /// ```
    pub fn watch<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherit::<T>()
    }

    /// Get InheritedWidget without dependency (short alias)
    ///
    /// Alternative name for `read_inherited()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme = context.read::<Theme>();  // Like React hooks!
    /// ```
    pub fn read<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.read_inherited::<T>()
    }

    // ========== Phase 6: InheritedModel Support ==========

    /// Access InheritedWidget with specific aspect dependency
    ///
    /// Used by InheritedModel to register aspect-based dependencies.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme = context.depend_on_inherited_widget_of_exact_type_with_aspect::<AppTheme>(
    ///     Some(Box::new(ThemeAspect::PrimaryColor))
    /// );
    /// ```
    pub fn depend_on_inherited_widget_of_exact_type_with_aspect<T>(
        &self,
        aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        // Find inherited element
        let tree = self.tree.read();
        let type_id = TypeId::of::<T>();

        let ancestor_id = self
            .ancestors()
            .find(|&ancestor_id| {
                if let Some(element) = tree.get(ancestor_id) {
                    element.widget_has_type_id(type_id)
                } else {
                    false
                }
            })?;

        // Register dependency with aspect
        self.depend_on_inherited_element(ancestor_id, aspect);

        // Get widget value
        let element = tree.get(ancestor_id)?;
        element
            .widget_as_any()
            .and_then(|any| any.downcast_ref::<T>())
            .cloned()
    }
}

