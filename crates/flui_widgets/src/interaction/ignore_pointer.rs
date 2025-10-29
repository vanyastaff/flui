//! IgnorePointer widget - makes widget transparent to pointer events
//!
//! A widget that is invisible to pointer events.
//! Similar to Flutter's IgnorePointer widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! IgnorePointer {
//!     ignoring: true,
//!     child: Some(Box::new(some_widget)),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! IgnorePointer::builder()
//!     .ignoring(true)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::{RenderIgnorePointer, SingleArity};

/// A widget that is invisible to pointer events.
///
/// When `ignoring` is true, this widget and its subtree will not receive pointer events.
/// Events will pass through to widgets behind it.
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - No effect on layout, only affects hit testing
///
/// ## Hit Testing Behavior
///
/// - When `ignoring` is true: Widget is transparent to hit tests
///   (events pass through to widgets behind)
/// - When `ignoring` is false: Normal hit testing
///
/// ## Difference from AbsorbPointer
///
/// - **IgnorePointer**: Transparent - events pass through to widgets behind
/// - **AbsorbPointer**: Opaque - events are blocked from reaching widgets behind
///
/// ## Examples
///
/// ```rust,ignore
/// // Make a button non-interactive
/// IgnorePointer::builder()
///     .ignoring(true)
///     .child(Button::new("Can't click me"))
///     .build()
///
/// // Conditionally ignore events
/// IgnorePointer::builder()
///     .ignoring(is_disabled)
///     .child(some_widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_ignore_pointer)]
pub struct IgnorePointer {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Whether to ignore pointer events.
    ///
    /// When true, this widget and its subtree are invisible to hit tests.
    /// Events will pass through to widgets behind.
    #[builder(default = true)]
    pub ignoring: bool,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl IgnorePointer {
    /// Creates a new IgnorePointer widget.
    ///
    /// # Parameters
    ///
    /// - `ignoring`: Whether to ignore pointer events (default: true)
    pub fn new(ignoring: bool) -> Self {
        Self {
            key: None,
            ignoring,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child<W>(&mut self, child: W)
    where
        W: Widget + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        self.child = Some(BoxedWidget::new(child));
    }
}

impl Default for IgnorePointer {
    fn default() -> Self {
        Self::new(true)
    }
}

// Implement Widget trait with associated type


// bon Builder Extensions
use ignore_pointer_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> IgnorePointerBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child<W: Widget + 'static>(self, child: W) -> IgnorePointerBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Build wrapper
impl<S: State> IgnorePointerBuilder<S> {
    /// Builds the IgnorePointer widget.
    pub fn build(self) -> IgnorePointer {
        self.build_ignore_pointer()
    }
}

/// Macro for creating IgnorePointer with declarative syntax.
#[macro_export]
macro_rules! ignore_pointer {
    () => {
        $crate::IgnorePointer::new(true)
    };
    (ignoring: $ignoring:expr) => {
        $crate::IgnorePointer::new($ignoring)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    #[derive(Debug, Clone)]
    struct MockWidget;

    

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

    #[test]
    fn test_ignore_pointer_new() {
        let widget = IgnorePointer::new(true);
        assert!(widget.key.is_none());
        assert!(widget.ignoring);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_ignore_pointer_new_false() {
        let widget = IgnorePointer::new(false);
        assert!(!widget.ignoring);
    }

    #[test]
    fn test_ignore_pointer_default() {
        let widget = IgnorePointer::default();
        assert!(widget.ignoring);
    }

    #[test]
    fn test_ignore_pointer_builder() {
        let widget = IgnorePointer::builder().build();
        assert!(widget.ignoring); // Default is true
    }

    #[test]
    fn test_ignore_pointer_builder_with_child() {
        let widget = IgnorePointer::builder().child(MockWidget).build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_ignore_pointer_builder_with_ignoring_false() {
        let widget = IgnorePointer::builder().ignoring(false).build();
        assert!(!widget.ignoring);
    }

    #[test]
    fn test_ignore_pointer_set_child() {
        let mut widget = IgnorePointer::new(true);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_ignore_pointer_macro_default() {
        let widget = ignore_pointer!();
        assert!(widget.ignoring);
    }

    #[test]
    fn test_ignore_pointer_macro_with_value() {
        let widget = ignore_pointer!(ignoring: false);
        assert!(!widget.ignoring);
    }

    #[test]
    fn test_ignore_pointer_widget_trait() {
        let widget = IgnorePointer::builder()
            .ignoring(true)
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_single_child_render_object_widget_trait() {
        let widget = IgnorePointer::builder()
            .ignoring(false)
            .child(MockWidget)
            .build();

        // Test child() method
        assert!(widget.child().is_some());
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for IgnorePointer {
    type RenderObject = RenderIgnorePointer;
    type Arity = SingleArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderIgnorePointer::new(self.ignoring)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_ignoring(self.ignoring);
    }
}

impl SingleChildRenderObjectWidget for IgnorePointer {
    fn child(&self) -> &BoxedWidget {
        self.child
            .as_ref()
            .unwrap_or_else(|| panic!("IgnorePointer requires a child"))
    }
}
