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
//!     child: Child::new(some_widget),
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
use flui_core::render::RenderBoxExt;
use flui_core::view::children::Child;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::RenderIgnorePointer;

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
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
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
    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
}

impl std::fmt::Debug for IgnorePointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IgnorePointer")
            .field("key", &self.key)
            .field("ignoring", &self.ignoring)
            .field(
                "child",
                &if self.child.is_some() {
                    "<child>"
                } else {
                    "None"
                },
            )
            .finish()
    }
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
            child: Child::none(),
        }
    }

    /// Creates an IgnorePointer with a child widget.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// IgnorePointer::with_child(true, Button::new("Disabled"))
    /// ```
    pub fn with_child(ignoring: bool, child: impl IntoElement) -> Self {
        Self::builder().ignoring(ignoring).child(child).build()
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl IntoElement) {
        self.child = Child::new(child);
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
    pub fn child(self, child: impl IntoElement) -> IgnorePointerBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

// Build wrapper
impl<S: State> IgnorePointerBuilder<S> {
    /// Builds the IgnorePointer widget.
    pub fn build(self) -> IgnorePointer {
        self.build_internal()
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
        let widget = IgnorePointer::builder()
            .child(crate::SizedBox::new())
            .build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_ignore_pointer_builder_with_ignoring_false() {
        let widget = IgnorePointer::builder().ignoring(false).build();
        assert!(!widget.ignoring);
    }

    #[test]
    fn test_ignore_pointer_with_child() {
        let widget = IgnorePointer::with_child(true, crate::SizedBox::new());
        assert!(widget.ignoring);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_ignore_pointer_set_child() {
        let mut widget = IgnorePointer::new(true);
        widget.set_child(crate::SizedBox::new());
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
}

// Implement View trait
impl StatelessView for IgnorePointer {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderIgnorePointer::new(self.ignoring).child_opt(self.child.into_inner())
    }
}
