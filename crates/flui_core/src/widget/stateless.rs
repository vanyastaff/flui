//! StatelessWidget - immutable widgets that build once
//!
//! Stateless widgets don't hold any mutable state - all configuration comes from
//! their fields which are immutable.

use std::fmt;

use super::{Widget, DynWidget, BoxedWidget, sealed};
use crate::element::ComponentElement;

/// StatelessWidget - immutable widget that builds once
///
/// Stateless widgets don't hold any mutable state - all configuration comes from
/// their fields which are immutable.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{StatelessWidget, BoxedWidget};
///
/// #[derive(Debug, Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessWidget for Greeting {
///     fn build(&self) -> BoxedWidget {
///         Box::new(Text::new(format!("Hello, {}!", self.name)))
///     }
/// }
/// ```
///
/// # Automatic Implementations
///
/// When you implement `StatelessWidget`, you automatically get implementations for:
/// - `Widget` trait (base widget trait)
/// - `DynWidget` trait (for heterogeneous storage)
///
/// This is done via blanket implementations in this module.
pub trait StatelessWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Build this widget's child widget tree
    ///
    /// Called when the widget is first built or when it needs to rebuild.
    /// Should return the root widget of the child tree.
    fn build(&self) -> BoxedWidget;
}

// ========== Automatic Implementations ==========

/// Automatically implement sealed::Sealed for all StatelessWidgets
///
/// This makes StatelessWidget types eligible for the Widget trait.
/// The ElementType is set to ComponentElement<T>.
impl<T: StatelessWidget> sealed::Sealed for T {
    type ElementType = ComponentElement<T>;
}

/// Automatically implement Widget for all StatelessWidgets
///
/// Thanks to the sealed trait pattern, this blanket impl doesn't conflict
/// with other widget type implementations.
impl<T: StatelessWidget> Widget for T {
    fn key(&self) -> Option<&str> {
        None
    }

    fn into_element(self) -> ComponentElement<T> {
        ComponentElement::new(self)
    }
}

/// Automatically implement DynWidget for all StatelessWidgets
impl<T: StatelessWidget> DynWidget for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
