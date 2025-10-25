//! InheritedWidget - widgets that propagate data down the tree
//!
//! InheritedWidgets allow data to be efficiently shared with descendant widgets.
//! When an InheritedWidget changes, only widgets that depend on it are rebuilt.

use std::fmt;
use super::{BoxedWidget, Widget, DynWidget, sealed};
use crate::element::InheritedElement;

/// InheritedWidget - widget that provides data to descendants
///
/// InheritedWidgets are a way to propagate information down the tree efficiently.
/// Descendant widgets can access the InheritedWidget and optionally register a
/// dependency so they rebuild when the data changes.
///
/// # Architecture
///
/// ```text
/// InheritedWidget
///   ↓
/// InheritedElement (stores dependents: HashSet<ElementId>)
///   ↓
/// Child widget tree (can access via context.depend_on::<T>())
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{InheritedWidget, BoxedWidget};
///
/// #[derive(Debug, Clone)]
/// struct Theme {
///     primary_color: Color,
///     text_size: f32,
/// }
///
/// impl InheritedWidget for Theme {
///     fn update_should_notify(&self, old: &Self) -> bool {
///         self.primary_color != old.primary_color ||
///         self.text_size != old.text_size
///     }
///
///     fn child(&self) -> BoxedWidget {
///         // Child widget that can access theme via context
///         Box::new(MyApp)
///     }
/// }
///
/// // Use macro to implement Widget + DynWidget
/// impl_widget_for_inherited!(Theme);
/// ```
///
/// # Accessing from descendants
///
/// ```rust,ignore
/// // With dependency (auto-rebuild on change)
/// let theme = context.depend_on::<Theme>()?;
///
/// // Without dependency (one-time read)
/// let theme = context.read::<Theme>()?;
/// ```
///
/// # Performance
///
/// - Only widgets that call `depend_on()` are notified of changes
/// - `update_should_notify()` controls when dependents rebuild
/// - Efficient: O(1) lookup up the ancestor chain
use super::DynWidget;

pub trait InheritedWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Check if dependents should be notified of changes
    ///
    /// Called when the InheritedWidget is updated with new data.
    /// Return `true` to rebuild all dependent widgets, `false` to skip.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn update_should_notify(&self, old: &Self) -> bool {
    ///     // Only notify if color changed, ignore other fields
    ///     self.primary_color != old.primary_color
    /// }
    /// ```
    fn update_should_notify(&self, old: &Self) -> bool;

    /// Get the child widget
    ///
    /// InheritedWidgets always have exactly one child that forms the subtree
    /// where this data is available.
    fn child(&self) -> BoxedWidget;
}

// ========== Automatic Implementations ==========

/// Automatically implement sealed::Sealed for all InheritedWidgets
///
/// This makes InheritedWidget types eligible for the Widget trait.
/// The ElementType is set to InheritedElement<T>.
impl<T: InheritedWidget> sealed::Sealed for T {
    type ElementType = InheritedElement<T>;
}

/// Automatically implement Widget for all InheritedWidgets
///
/// Thanks to the sealed trait pattern, this blanket impl doesn't conflict
/// with other widget type implementations.
impl<T: InheritedWidget> Widget for T {
    fn key(&self) -> Option<&str> {
        None
    }

    fn into_element(self) -> InheritedElement<T> {
        InheritedElement::new(self)
    }
}

/// Automatically implement DynWidget for all InheritedWidgets
impl<T: InheritedWidget> DynWidget for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
