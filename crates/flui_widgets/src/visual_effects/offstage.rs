//! Offstage widget - hides widget without removing it from tree
//!
//! A widget that lays out its child as if it was in the tree, but without painting it.
//! Similar to Flutter's Offstage widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! Offstage::builder()
//!     .offstage(true)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, RenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderOffstage;

/// A widget that lays out its child as if it was in the tree, but without painting or hit testing.
///
/// When `offstage` is true:
/// - The child is NOT painted (invisible)
/// - The child is NOT hit tested (doesn't receive pointer events)
/// - The child IS still laid out (maintains its size and state)
///
/// ## Use Cases
///
/// - **Preserving State**: Keep a widget's state while hiding it
/// - **Animation**: Smoothly animate visibility without rebuilding
/// - **Performance**: Avoid rebuilding expensive widgets when showing/hiding
/// - **Conditional Display**: Toggle visibility without changing the widget tree
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - Child is always laid out, even when offstage
///
/// ## Difference from Visibility Widget
///
/// - **Offstage**: Child is laid out but not painted (takes up space)
/// - **Visibility (gone)**: Child is not laid out and not painted (no space)
///
/// ## Examples
///
/// ```rust,ignore
/// // Hide a widget while preserving its state
/// Offstage::builder()
///     .offstage(is_hidden)
///     .child(ExpensiveWidget::new())
///     .build()
///
/// // Toggle visibility
/// Offstage::builder()
///     .offstage(!is_visible)
///     .child(content)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Offstage {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Whether the child is offstage (hidden).
    ///
    /// When true, child is laid out but not painted or hit tested.
    #[builder(default = true)]
    pub offstage: bool,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Offstage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offstage")
            .field("key", &self.key)
            .field("offstage", &self.offstage)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for Offstage {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            offstage: self.offstage,
            child: self.child.clone(),
        }
    }
}

impl Offstage {
    /// Creates a new Offstage widget.
    ///
    /// # Parameters
    ///
    /// - `offstage`: If true, child is hidden (default: true)
    pub fn new(offstage: bool) -> Self {
        Self {
            key: None,
            offstage,
            child: None,
        }
    }

    /// Creates an Offstage widget with a child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Offstage::with_child(true, ExpensiveWidget::new())
    /// ```
    pub fn with_child(offstage: bool, child: impl View + 'static) -> Self {
        Self::builder().offstage(offstage).child(child).build()
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Box<dyn AnyView>) {
        self.child = Some(child);
    }
}

impl Default for Offstage {
    fn default() -> Self {
        Self::new(true)
    }
}

// Implement Widget trait with associated type

// bon Builder Extensions
use offstage_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> OffstageBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> OffstageBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper
impl<S: State> OffstageBuilder<S> {
    /// Builds the Offstage widget.
    pub fn build(self) -> Offstage {
        self.build_internal()
    }
}

// Implement View trait
impl View for Offstage {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderBuilder::single(RenderOffstage::new(self.offstage)).with_optional_child(self.child)
    }
}

/// Macro for creating Offstage with declarative syntax.
#[macro_export]
macro_rules! offstage {
    () => {
        $crate::Offstage::new(true)
    };
    (offstage: $offstage:expr) => {
        $crate::Offstage::new($offstage)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offstage_new() {
        let widget = Offstage::new(true);
        assert!(widget.key.is_none());
        assert!(widget.offstage);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_offstage_new_false() {
        let widget = Offstage::new(false);
        assert!(!widget.offstage);
    }

    #[test]
    fn test_offstage_default() {
        let widget = Offstage::default();
        assert!(widget.offstage);
    }

    #[test]
    fn test_offstage_builder() {
        let widget = Offstage::builder().build();
        assert!(widget.offstage); // Default is true
    }

    #[test]
    fn test_offstage_builder_with_child() {
        let widget = Offstage::builder().child(crate::SizedBox::new()).build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_offstage_builder_with_offstage_false() {
        let widget = Offstage::builder().offstage(false).build();
        assert!(!widget.offstage);
    }

    #[test]
    fn test_offstage_with_child() {
        let widget = Offstage::with_child(true, crate::SizedBox::new());
        assert!(widget.offstage);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_offstage_set_child() {
        let mut widget = Offstage::new(true);
        widget.set_child(Box::new(crate::SizedBox::new()));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_offstage_macro_default() {
        let widget = offstage!();
        assert!(widget.offstage);
    }

    #[test]
    fn test_offstage_macro_with_value() {
        let widget = offstage!(offstage: false);
        assert!(!widget.offstage);
    }
}
