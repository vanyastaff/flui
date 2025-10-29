//! AbsorbPointer widget - absorbs pointer events preventing them from passing through
//!
//! A widget that absorbs pointer events during hit testing.
//! Similar to Flutter's AbsorbPointer widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! AbsorbPointer {
//!     absorbing: true,
//!     child: Some(Box::new(some_widget)),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! AbsorbPointer::builder()
//!     .absorbing(true)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::{RenderAbsorbPointer, SingleArity};

/// A widget that absorbs pointer events during hit testing.
///
/// When `absorbing` is true, this widget prevents its subtree from receiving pointer events
/// and prevents events from passing through to widgets behind it.
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - No effect on layout, only affects hit testing
///
/// ## Hit Testing Behavior
///
/// - When `absorbing` is true: Widget blocks hit tests
///   (events don't pass through to widgets behind and child doesn't receive them)
/// - When `absorbing` is false: Normal hit testing
///
/// ## Difference from IgnorePointer
///
/// - **IgnorePointer**: Transparent - events pass through to widgets behind
/// - **AbsorbPointer**: Opaque - events are blocked from reaching widgets behind
///
/// ## Examples
///
/// ```rust,ignore
/// // Block all pointer events to widgets behind
/// AbsorbPointer::builder()
///     .absorbing(true)
///     .child(Button::new("This button won't work"))
///     .build()
///
/// // Conditionally block events
/// AbsorbPointer::builder()
///     .absorbing(is_loading)
///     .child(content_widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_absorb_pointer)]
pub struct AbsorbPointer {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Whether to absorb pointer events.
    ///
    /// When true, this widget blocks hit tests and prevents events
    /// from reaching both its children and widgets behind it.
    #[builder(default = true)]
    pub absorbing: bool,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl AbsorbPointer {
    /// Creates a new AbsorbPointer widget.
    ///
    /// # Parameters
    ///
    /// - `absorbing`: Whether to absorb pointer events (default: true)
    pub fn new(absorbing: bool) -> Self {
        Self {
            key: None,
            absorbing,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child<W: Widget + 'static>(&mut self, child: W) {
        self.child = Some(BoxedWidget::new(child));
    }
}

impl Default for AbsorbPointer {
    fn default() -> Self {
        Self::new(true)
    }
}

// Implement Widget trait with associated type


// bon Builder Extensions
use absorb_pointer_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> AbsorbPointerBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child<W: Widget + 'static>(self, child: W) -> AbsorbPointerBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Build wrapper
impl<S: State> AbsorbPointerBuilder<S> {
    /// Builds the AbsorbPointer widget.
    pub fn build(self) -> AbsorbPointer {
        self.build_absorb_pointer()
    }
}

/// Macro for creating AbsorbPointer with declarative syntax.
#[macro_export]
macro_rules! absorb_pointer {
    () => {
        $crate::AbsorbPointer::new(true)
    };
    (absorbing: $absorbing:expr) => {
        $crate::AbsorbPointer::new($absorbing)
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
    fn test_absorb_pointer_new() {
        let widget = AbsorbPointer::new(true);
        assert!(widget.key.is_none());
        assert!(widget.absorbing);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_absorb_pointer_new_false() {
        let widget = AbsorbPointer::new(false);
        assert!(!widget.absorbing);
    }

    #[test]
    fn test_absorb_pointer_default() {
        let widget = AbsorbPointer::default();
        assert!(widget.absorbing);
    }

    #[test]
    fn test_absorb_pointer_builder() {
        let widget = AbsorbPointer::builder().build();
        assert!(widget.absorbing); // Default is true
    }

    #[test]
    fn test_absorb_pointer_builder_with_child() {
        let widget = AbsorbPointer::builder().child(MockWidget).build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_absorb_pointer_builder_with_absorbing_false() {
        let widget = AbsorbPointer::builder().absorbing(false).build();
        assert!(!widget.absorbing);
    }

    #[test]
    fn test_absorb_pointer_set_child() {
        let mut widget = AbsorbPointer::new(true);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_absorb_pointer_macro_default() {
        let widget = absorb_pointer!();
        assert!(widget.absorbing);
    }

    #[test]
    fn test_absorb_pointer_macro_with_value() {
        let widget = absorb_pointer!(absorbing: false);
        assert!(!widget.absorbing);
    }

    #[test]
    fn test_absorb_pointer_widget_trait() {
        let widget = AbsorbPointer::builder()
            .absorbing(true)
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_single_child_render_object_widget_trait() {
        let widget = AbsorbPointer::builder()
            .absorbing(false)
            .child(MockWidget)
            .build();

        // Test child() method
        assert!(widget.child().is_some());
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for AbsorbPointer {
    type RenderObject = RenderAbsorbPointer;
    type Arity = SingleArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderAbsorbPointer::new(self.absorbing)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_absorbing(self.absorbing);
    }
}

impl SingleChildRenderObjectWidget for AbsorbPointer {
    fn child(&self) -> &BoxedWidget {
        self.child
            .as_ref()
            .unwrap_or_else(|| panic!("AbsorbPointer requires a child"))
    }
}
