//! Widget Kind - Type-level discrimination for Widget types
//!
//! This module provides a type-safe way to distinguish between different kinds
//! of widgets using associated types, allowing blanket implementations without conflicts.
//!
//! # Architecture
//!
//! The `WidgetKind` system enables us to have multiple blanket implementations of
//! `Widget` trait without conflicts:
//!
//! ```text
//! impl<T: StatelessWidget> Widget for T { type Kind = ComponentKind; }
//! impl<T: StatefulWidget> Widget for T { type Kind = StatefulKind; }
//! impl<T: InheritedWidget> Widget for T { type Kind = InheritedKind; }
//! impl<W, T: ParentData> Widget for W where W: ParentDataWidget<T> { type Kind = ParentDataKind; }
//! ```
//!
//! These implementations don't conflict because each has a different `Kind` type.
//!
//! # Sealed Trait Pattern
//!
//! The `WidgetKind` trait is sealed to prevent external implementations.
//! Only the 5 predefined kinds can exist.

mod sealed {
    /// Sealed trait to prevent external WidgetKind implementations
    pub trait WidgetKind: 'static {}
}

/// Marker trait for widget type discrimination
///
/// This trait is sealed and can only be implemented by the predefined
/// widget kinds in this module.
pub trait WidgetKind: sealed::WidgetKind {}

// ========== Widget Kinds ==========

/// Widget kind for StatelessWidget
///
/// StatelessWidget builds a child widget tree once based on immutable configuration.
///
/// # Example
///
/// ```rust,ignore
/// impl<T: StatelessWidget> Widget for T {
///     type Kind = ComponentKind;  // ← This kind
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentKind;

impl sealed::WidgetKind for ComponentKind {}
impl WidgetKind for ComponentKind {}

/// Widget kind for StatefulWidget
///
/// StatefulWidget creates a mutable State object that persists across rebuilds.
///
/// # Example
///
/// ```rust,ignore
/// impl<T: StatefulWidget> Widget for T {
///     type Kind = StatefulKind;  // ← This kind
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatefulKind;

impl sealed::WidgetKind for StatefulKind {}
impl WidgetKind for StatefulKind {}

/// Widget kind for InheritedWidget
///
/// InheritedWidget propagates data down the widget tree with dependency tracking.
///
/// # Example
///
/// ```rust,ignore
/// impl<T: InheritedWidget> Widget for T {
///     type Kind = InheritedKind;  // ← This kind
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InheritedKind;

impl sealed::WidgetKind for InheritedKind {}
impl WidgetKind for InheritedKind {}

/// Widget kind for ParentDataWidget
///
/// ParentDataWidget attaches metadata to descendant RenderObjects for layout purposes.
///
/// # Example
///
/// ```rust,ignore
/// impl<W, T> Widget for W
/// where
///     W: ParentDataWidget<T>,
///     T: ParentData,
/// {
///     type Kind = ParentDataKind;  // ← This kind
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentDataKind;

impl sealed::WidgetKind for ParentDataKind {}
impl WidgetKind for ParentDataKind {}

/// Widget kind for RenderObjectWidget
///
/// RenderObjectWidget directly creates RenderObjects for layout and painting.
///
/// # Example
///
/// ```rust,ignore
/// impl Widget for MyRenderWidget {
///     type Kind = RenderObjectKind;  // ← This kind (manual impl)
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderObjectKind;

impl sealed::WidgetKind for RenderObjectKind {}
impl WidgetKind for RenderObjectKind {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_kinds_exist() {
        let _component = ComponentKind;
        let _stateful = StatefulKind;
        let _inherited = InheritedKind;
        let _parent_data = ParentDataKind;
        let _render_object = RenderObjectKind;
    }

    #[test]
    fn test_widget_kinds_equality() {
        assert_eq!(ComponentKind, ComponentKind);
        assert_ne!(ComponentKind, StatefulKind);
        assert_ne!(InheritedKind, ParentDataKind);
    }

    #[test]
    fn test_widget_kinds_copy() {
        let k1 = ComponentKind;
        let k2 = k1; // Copy
        assert_eq!(k1, k2);
    }
}
