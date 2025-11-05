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
use flui_core::view::{AnyView, ChangeFlags, View};
use flui_core::render::RenderNode;
use flui_core::{BuildContext, Element};
use flui_rendering::RenderAbsorbPointer;

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
#[derive(Builder)]
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
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for AbsorbPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AbsorbPointer")
            .field("key", &self.key)
            .field("absorbing", &self.absorbing)
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for AbsorbPointer {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            absorbing: self.absorbing,
            child: None,
        }
    }
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
    pub fn set_child(&mut self, child: Box<dyn AnyView>) {
        self.child = Some(child);
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
    pub fn child(self, child: impl View + 'static) -> AbsorbPointerBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
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
        let widget = AbsorbPointer::builder().build_absorb_pointer();
        assert!(widget.absorbing); // Default is true
    }

    #[test]
    fn test_absorb_pointer_builder_with_child() {
        let widget = AbsorbPointer::builder()
            .child(crate::SizedBox::new())
            .build_absorb_pointer();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_absorb_pointer_builder_with_absorbing_false() {
        let widget = AbsorbPointer::builder()
            .absorbing(false)
            .build_absorb_pointer();
        assert!(!widget.absorbing);
    }

    #[test]
    fn test_absorb_pointer_set_child() {
        let mut widget = AbsorbPointer::new(true);
        widget.set_child(Box::new(crate::SizedBox::new()));
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
}

// Implement View trait
impl View for AbsorbPointer {
    type Element = Element;
    type State = Option<Box<dyn std::any::Any>>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child first
        let (child_id, child_state) = if let Some(child) = self.child {
            let (elem, state) = child.build_any(ctx);
            let id = ctx.tree().write().insert(elem.into_element());
            (Some(id), Some(state))
        } else {
            (None, None)
        };

        // Create RenderAbsorbPointer
        let render = RenderAbsorbPointer::new(self.absorbing);

        let render_node = RenderNode::Single {
            render: Box::new(render),
            child: child_id,
        };

        let render_element = flui_core::element::RenderElement::new(render_node);
        (Element::Render(render_element), child_state)
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // TODO: Implement proper rebuild logic if needed
        // For now, return NONE as View architecture handles rebuilding
        ChangeFlags::NONE
    }
}
