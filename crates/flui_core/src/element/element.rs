//! Element trait - base trait for all elements

use crate::widget::Widget;

/// Base Element trait for widget instances
///
/// Elements are the mutable state holders in the three-tree architecture.
/// They persist across rebuilds and manage the lifecycle of widgets.
pub trait Element<W: Widget + ?Sized>: 'static {
    /// Create a new element from a widget
    fn new(widget: W) -> Self
    where
        W: Sized;
}
