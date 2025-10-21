//! InheritedWidget access methods for Context
//!
//! Provides clean, ergonomic APIs for accessing InheritedWidgets from the element tree,
//! with or without establishing dependencies for rebuild notifications.
//!
//! # Recommended API
//!
//! Use the short, ergonomic methods for the best experience:
//!
//! ```rust,ignore
//! // With dependency (auto-rebuild on change)
//! let theme = context.inherit::<Theme>()?;
//! let theme = context.watch::<Theme>()?;     // React-style alias
//!
//! // Without dependency (one-time read)
//! let theme = context.read::<Theme>()?;
//!
//! // With aspect (for InheritedModel)
//! let theme = context.inherit_aspect::<Theme>(Some(Box::new(ThemeAspect::Color)))?;
//! ```
//!
//! # Legacy API
//!
//! For compatibility with macros, these methods are also available:
//!
//! ```rust,ignore
//! let theme = context.depend_on_inherited_widget::<Theme>()?;
//! let theme = context.get_inherited_widget::<Theme>()?;
//! ```

use std::any::TypeId;
use crate::ElementId;
use crate::widget::InheritedWidget;
use super::Context;

// =============================================================================
// Modern Type-Safe API (Recommended)
// =============================================================================

impl Context {
    /// Accesses an InheritedWidget and establishes dependency
    ///
    /// This is the **primary API** for accessing InheritedWidgets. When the
    /// widget changes, the current element will automatically rebuild.
    ///
    /// # Returns
    ///
    /// A cloned widget if found, `None` if no ancestor has this type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Simple access with auto-rebuild
    /// let theme = context.inherit::<Theme>().unwrap();
    /// println!("Theme color: {:?}", theme.primary_color);
    ///
    /// // With unwrap_or_default
    /// let theme = context.inherit::<Theme>().unwrap_or_default();
    /// ```
    #[must_use]
    pub fn inherit<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherited_widget_impl::<T>(true, None)
    }

    /// Accesses an InheritedWidget with React-style naming
    ///
    /// Alias for [`inherit`](Self::inherit), inspired by React hooks.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let theme = context.watch::<Theme>()?;  // Like React's useContext
    /// ```
    #[must_use]
    #[inline]
    pub fn watch<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherit::<T>()
    }

    /// Accesses an InheritedWidget without dependency
    ///
    /// Use this when you only need to **read once** without automatic rebuilds.
    /// The element will NOT be notified when the widget changes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // One-time read at initialization
    /// let theme = context.read::<Theme>()?;
    /// println!("Initial theme: {:?}", theme.name);
    /// ```
    #[must_use]
    pub fn read<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherited_widget_impl::<T>(false, None)
    }

    /// Accesses an InheritedWidget with a specific aspect dependency
    ///
    /// Used by InheritedModel to register aspect-based dependencies.
    /// Only rebuilds when the specified aspect of the widget changes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::any::Any;
    ///
    /// #[derive(Debug)]
    /// enum ThemeAspect {
    ///     PrimaryColor,
    ///     Typography,
    /// }
    ///
    /// let theme = context.inherit_aspect::<AppTheme>(
    ///     Some(Box::new(ThemeAspect::PrimaryColor))
    /// )?;
    /// // Only rebuilds when PrimaryColor aspect changes
    /// ```
    #[must_use]
    pub fn inherit_aspect<T>(
        &self,
        aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherited_widget_impl::<T>(true, aspect)
    }
}

// =============================================================================
// Element-Level Access
// =============================================================================

impl Context {
    /// Finds the element ID for an inherited widget
    ///
    /// Low-level API that returns the ElementId rather than the widget.
    /// Useful for advanced use cases or debugging.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(theme_id) = context.inherited_element::<Theme>() {
    ///     println!("Theme element: {:?}", theme_id);
    /// }
    /// ```
    #[must_use]
    pub fn inherited_element<W>(&self) -> Option<ElementId>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        use crate::widget::InheritedElement;

        let tree = self.tree();

        self.ancestors().find(|&id| {
            tree.get(id)
                .map(|elem| elem.is::<InheritedElement<W>>())
                .unwrap_or(false)
        })
    }
}

// =============================================================================
// Legacy Macro-Based API (For Compatibility)
// =============================================================================

impl Context {
    /// Accesses InheritedWidget with dependency (legacy, for macros)
    ///
    /// Used with the `impl_widget_for_inherited!` macro.
    /// For direct usage, prefer [`inherit()`](Self::inherit).
    ///
    /// Note: W must implement a Widget with `Element = InheritedElement<W>`.
    #[must_use]
    pub fn depend_on_inherited_widget<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.legacy_inherited_widget_impl::<W>(true)
    }

    /// Accesses InheritedWidget with dependency (legacy alias)
    #[must_use]
    #[inline]
    pub fn subscribe_to<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.depend_on_inherited_widget()
    }

    /// Accesses InheritedWidget without dependency (legacy, for macros)
    ///
    /// For direct usage, prefer [`read()`](Self::read).
    #[must_use]
    pub fn get_inherited_widget<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.legacy_inherited_widget_impl::<W>(false)
    }

    /// Accesses InheritedWidget without dependency (legacy alias)
    #[must_use]
    #[inline]
    pub fn find_inherited<W>(&self) -> Option<W>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.get_inherited_widget()
    }
}

// =============================================================================
// Internal Implementation
// =============================================================================

impl Context {
    /// Finds ancestor InheritedElement of type T
    ///
    /// Internal helper that uses TypeId for efficient type checking.
    fn find_inherited_element_by_type<T>(&self) -> Option<ElementId>
    where
        T: InheritedWidget + 'static,
    {
        let tree = self.tree();
        let target_type_id = TypeId::of::<T>();

        self.ancestors().find(|&ancestor_id| {
            tree.get(ancestor_id)
                .map(|elem| elem.widget_has_type_id(target_type_id))
                .unwrap_or(false)
        })
    }

    /// Registers a dependency on an InheritedElement
    ///
    /// Low-level method used internally. Prefer typed methods.
    fn register_dependency(
        &self,
        ancestor_id: ElementId,
        aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) {
        let mut tree = self.tree_mut();
        if let Some(ancestor) = tree.get_mut(ancestor_id) {
            ancestor.register_dependency(self.element_id(), aspect);
        }
    }

    /// Modern implementation for type-safe InheritedWidget access
    ///
    /// This is the core implementation used by the public APIs.
    fn inherited_widget_impl<T>(
        &self,
        register_dependency: bool,
        aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        // Find the inherited element
        let ancestor_id = self.find_inherited_element_by_type::<T>()?;

        // Register dependency if requested
        if register_dependency {
            self.register_dependency(ancestor_id, aspect);
        }

        // Get and clone the widget
        let tree = self.tree();
        tree.get(ancestor_id)
            .and_then(|element| element.widget_as_any())
            .and_then(|any| any.downcast_ref::<T>())
            .cloned()
    }

    /// Legacy implementation for macro-based InheritedWidget access
    ///
    /// This handles the complex locking logic needed with InheritedElement<W>.
    fn legacy_inherited_widget_impl<W>(
        &self,
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

                    return if register_dependency {
                        // Drop read lock before acquiring write lock
                        drop(tree);

                        // Register dependency
                        let mut tree_mut = self.tree_mut();
                        if let Some(inherited_elem_mut) = tree_mut
                            .get_mut(id)
                            .and_then(|e| e.downcast_mut::<InheritedElement<W>>())
                        {
                            inherited_elem_mut.register_dependent(self.element_id());
                        }

                        // Re-acquire read lock to get a widget
                        let tree = self.tree();
                        tree
                            .get(id)
                            .and_then(|e| e.downcast_ref::<InheritedElement<W>>())
                            .map(|elem| elem.widget().clone())
                    } else {
                        // No dependency registration
                        Some(inherited_elem.widget().clone())
                    }
                }

                current_id = element.parent();
            } else {
                break;
            }
        }

        None
    }
}

// =============================================================================
// Deprecated Aliases (For Migration)
// =============================================================================

#[allow(deprecated)]
impl Context {
    /// Deprecated: use [`inherit()`](Self::inherit) instead
    #[deprecated(since = "0.2.0", note = "use `inherit()` instead")]
    #[must_use]
    #[inline]
    pub fn depend_on_inherited_widget_of_exact_type<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherit::<T>()
    }

    /// Deprecated: use [`read()`](Self::read) instead
    #[deprecated(since = "0.2.0", note = "use `read()` instead")]
    #[must_use]
    #[inline]
    pub fn get_inherited_widget_of_exact_type<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.read::<T>()
    }

    /// Deprecated: use [`inherit_aspect()`](Self::inherit_aspect) instead
    #[deprecated(since = "0.2.0", note = "use `inherit_aspect()` instead")]
    #[must_use]
    #[inline]
    pub fn depend_on_inherited_widget_of_exact_type_with_aspect<T>(
        &self,
        aspect: Option<Box<dyn std::any::Any + Send + Sync>>,
    ) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.inherit_aspect::<T>(aspect)
    }

    /// Deprecated: use [`inherited_element()`](Self::inherited_element) instead
    #[deprecated(since = "0.2.0", note = "use `inherited_element()` instead")]
    #[must_use]
    #[inline]
    pub fn get_element_for_inherited_widget_of_exact_type<W>(&self) -> Option<ElementId>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.inherited_element::<W>()
    }

    /// Deprecated: use [`inherited_element()`](Self::inherited_element) instead
    #[deprecated(since = "0.2.0", note = "use `inherited_element()` instead")]
    #[must_use]
    #[inline]
    pub fn find_inherited_element<W>(&self) -> Option<ElementId>
    where
        W: InheritedWidget + crate::Widget<Element = crate::widget::InheritedElement<W>> + Clone + 'static,
    {
        self.inherited_element::<W>()
    }

    /// Deprecated: use [`read()`](Self::read) instead
    #[deprecated(since = "0.2.0", note = "use `read()` instead")]
    #[must_use]
    #[inline]
    pub fn read_inherited<T>(&self) -> Option<T>
    where
        T: InheritedWidget + Clone + 'static,
    {
        self.read::<T>()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Fake widget for testing
    #[derive(Clone)]
    struct DummyWidget {
        value: i32,
    }

    impl InheritedWidget for DummyWidget {
        fn update_should_notify(&self, _old: &Self) -> bool {
            true
        }
    }

    #[test]
    fn test_api_consistency() {
        let context = Context::empty();

        // All these should be equivalent (if the widget existed)
        let _w1 = context.inherit::<DummyWidget>();
        let _w2 = context.watch::<DummyWidget>();
    }

    #[test]
    fn test_read_api() {
        let context = Context::empty();

        // Read without dependency
        let _w = context.read::<DummyWidget>();
    }

    #[test]
    fn test_aspect_api() {
        let context = Context::empty();

        // With aspect
        let _w = context.inherit_aspect::<DummyWidget>(None);
    }

    #[test]
    #[allow(deprecated)]
    fn test_deprecated_apis() {
        let context = Context::empty();

        // These should still work but emit warnings
        let _w1 = context.depend_on_inherited_widget_of_exact_type::<DummyWidget>();
        let _w2 = context.get_inherited_widget_of_exact_type::<DummyWidget>();
    }
}