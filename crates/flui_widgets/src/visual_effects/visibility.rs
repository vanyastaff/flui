//! Visibility widget - flexible control over child visibility
//!
//! A widget that controls the visibility of its child with fine-grained control.
//! Similar to Flutter's Visibility widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! Visibility::builder()
//!     .visible(false)
//!     .maintain_size(true)
//!     .maintain_state(true)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::RenderVisibility;

/// A widget that controls the visibility of its child.
///
/// When `visible` is false, the child can be hidden in different ways:
/// - Fully removed (default): No layout, no paint, no state preserved
/// - Maintain size: Takes up space but not painted
/// - Maintain state: Not painted but state preserved
/// - Maintain animation: Animations continue even when hidden
///
/// ## Use Cases
///
/// - **Conditional Display**: Show/hide widgets based on state
/// - **Performance**: Avoid rebuilding when toggling visibility
/// - **Smooth Transitions**: Maintain size during fade animations
/// - **State Preservation**: Keep widget state when hidden
///
/// ## Visibility Modes
///
/// ```text
/// visible=true:  Child is fully visible and interactive
///
/// visible=false combinations:
/// - maintain_size=false (default): Child removed, no space taken
/// - maintain_size=true: Child hidden but space reserved
/// - maintain_state=true: Child hidden but state kept alive
/// - maintain_animation=true: Animations continue running
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple hide/show
/// Visibility::builder()
///     .visible(is_logged_in)
///     .child(UserProfile::new())
///     .build()
///
/// // Maintain size for smooth transitions
/// Visibility::builder()
///     .visible(is_visible)
///     .maintain_size(true)
///     .child(FadeTransition::new(child))
///     .build()
///
/// // Maintain state while hidden
/// Visibility::builder()
///     .visible(is_visible)
///     .maintain_state(true)
///     .child(ExpensiveWidget::new())
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Visibility {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Whether the child is visible.
    ///
    /// When true, child is laid out and painted normally.
    /// When false, behavior depends on other maintain* flags.
    #[builder(default = true)]
    pub visible: bool,

    /// Whether to maintain the space occupied by the child when not visible.
    ///
    /// - false (default): Child is removed, no space taken
    /// - true: Child's size is maintained but it's not painted
    #[builder(default = false)]
    pub maintain_size: bool,

    /// Whether to maintain the state of the child when not visible.
    ///
    /// - false (default): Child is removed from tree, state lost
    /// - true: Child stays in tree, state preserved
    ///
    /// Note: If maintain_size=true, this is automatically true.
    #[builder(default = false)]
    pub maintain_state: bool,

    /// Whether to maintain animations when not visible.
    ///
    /// - false (default): Animations are stopped
    /// - true: Animations continue running
    ///
    /// Note: If maintain_size=true, this is automatically true.
    #[builder(default = false)]
    pub maintain_animation: bool,

    /// Whether to maintain interactivity when not visible.
    ///
    /// - false (default): Widget doesn't receive pointer events
    /// - true: Widget remains interactive even when invisible
    ///
    /// Usually kept false for safety.
    #[builder(default = false)]
    pub maintain_interactivity: bool,

    /// Whether to maintain semantics when not visible.
    ///
    /// - false (default): Widget excluded from semantics tree
    /// - true: Widget included in semantics (for accessibility)
    #[builder(default = false)]
    pub maintain_semantics: bool,

    /// The widget to show when child is not visible.
    ///
    /// Only used when visible=false and maintain_size=false.
    #[builder(setters(vis = "", name = replacement_internal))]
    pub replacement: Option<Box<dyn >>,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn >>,
}

impl std::fmt::Debug for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Visibility")
            .field("key", &self.key)
            .field("visible", &self.visible)
            .field("maintain_size", &self.maintain_size)
            .field("maintain_state", &self.maintain_state)
            .field("maintain_animation", &self.maintain_animation)
            .field("maintain_interactivity", &self.maintain_interactivity)
            .field("maintain_semantics", &self.maintain_semantics)
            .field(
                "replacement",
                &if self.replacement.is_some() {
                    "<>"
                } else {
                    "None"
                },
            )
            .field(
                "child",
                &if self.child.is_some() {
                    "<>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for Visibility {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            visible: self.visible,
            maintain_size: self.maintain_size,
            maintain_state: self.maintain_state,
            maintain_animation: self.maintain_animation,
            maintain_interactivity: self.maintain_interactivity,
            maintain_semantics: self.maintain_semantics,
            replacement: self.replacement.clone(),
            child: self.child.clone(),
        }
    }
}

impl Visibility {
    /// Creates a new Visibility widget.
    ///
    /// # Parameters
    ///
    /// - `visible`: Whether the child is visible (default: true)
    pub fn new(visible: bool) -> Self {
        Self {
            key: None,
            visible,
            maintain_size: false,
            maintain_state: false,
            maintain_animation: false,
            maintain_interactivity: false,
            maintain_semantics: false,
            replacement: None,
            child: None,
        }
    }

    /// Creates a Visibility widget with a child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Visibility::with_child(true, Text::new("Visible text"))
    /// ```
    pub fn with_child(visible: bool, child: impl IntoElement) -> Self {
        Self::builder().visible(visible).child(child).build()
    }

    /// Creates a Visibility widget that maintains size when hidden.
    ///
    /// Useful for fade animations where you don't want layout to jump.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Visibility::maintain_size(FadeTransition::new(child))
    /// ```
    pub fn maintain_size(child: impl IntoElement) -> Self {
        Self {
            key: None,
            visible: true,
            maintain_size: true,
            maintain_state: true,     // Auto-enable
            maintain_animation: true, // Auto-enable
            maintain_interactivity: false,
            maintain_semantics: false,
            replacement: None,
            child: Some(Box::new(child)),
        }
    }

    /// Creates a Visibility widget that maintains state when hidden.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Visibility::maintain_state_only(ExpensiveWidget::new())
    /// ```
    pub fn maintain_state_only(child: impl IntoElement) -> Self {
        Self {
            key: None,
            visible: true,
            maintain_size: false,
            maintain_state: true,
            maintain_animation: false,
            maintain_interactivity: false,
            maintain_semantics: false,
            replacement: None,
            child: Some(Box::new(child)),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Box<dyn >) {
        self.child = Some(child);
    }

    /// Sets the replacement widget.
    pub fn set_replacement(&mut self, replacement: Box<dyn >) {
        self.replacement = Some(replacement);
    }
}

impl Default for Visibility {
    fn default() -> Self {
        Self::new(true)
    }
}

// bon Builder Extensions
use visibility_builder::{IsUnset, SetChild, SetReplacement, State};

// Custom child setter
impl<S: State> VisibilityBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl IntoElement) -> VisibilityBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Custom replacement setter
impl<S: State> VisibilityBuilder<S>
where
    S::Replacement: IsUnset,
{
    /// Sets the replacement widget shown when child is not visible.
    pub fn replacement(
        self,
        replacement: impl IntoElement,
    ) -> VisibilityBuilder<SetReplacement<S>> {
        self.replacement_internal(Box::new(replacement))
    }
}

// Build wrapper
impl<S: State> VisibilityBuilder<S> {
    /// Builds the Visibility widget.
    pub fn build(self) -> Visibility {
        self.build_internal()
    }
}

// Implement View trait
impl StatelessView for Visibility {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        // Determine which child to show
        let child = if self.visible {
            self.child
        } else if !self.maintain_size && !self.maintain_state {
            // Fully replace with replacement widget if provided
            self.replacement.or(self.child)
        } else {
            // Maintain state/size: keep original child
            self.child
        };

        (
            RenderVisibility::new(
                self.visible,
                self.maintain_size,
                self.maintain_state,
                self.maintain_animation,
                self.maintain_interactivity,
                self.maintain_semantics,
            ),
            child,
        )
    }
}

/// Macro for creating Visibility with declarative syntax.
#[macro_export]
macro_rules! visibility {
    () => {
        $crate::Visibility::new(true)
    };
    (visible: $visible:expr) => {
        $crate::Visibility::new($visible)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visibility_new() {
        let widget = Visibility::new(true);
        assert!(widget.key.is_none());
        assert!(widget.visible);
        assert!(!widget.maintain_size);
        assert!(!widget.maintain_state);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_visibility_new_false() {
        let widget = Visibility::new(false);
        assert!(!widget.visible);
    }

    #[test]
    fn test_visibility_default() {
        let widget = Visibility::default();
        assert!(widget.visible);
    }

    #[test]
    fn test_visibility_maintain_size() {
        let widget = Visibility::maintain_size(crate::SizedBox::new());
        assert!(widget.maintain_size);
        assert!(widget.maintain_state); // Auto-enabled
        assert!(widget.maintain_animation); // Auto-enabled
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_visibility_maintain_state_only() {
        let widget = Visibility::maintain_state_only(crate::SizedBox::new());
        assert!(widget.maintain_state);
        assert!(!widget.maintain_size);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_visibility_with_child() {
        let widget = Visibility::with_child(false, crate::SizedBox::new());
        assert!(!widget.visible);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_visibility_builder() {
        let widget = Visibility::builder().build();
        assert!(widget.visible); // Default is true
    }

    #[test]
    fn test_visibility_builder_full() {
        let widget = Visibility::builder()
            .visible(false)
            .maintain_size(true)
            .maintain_state(true)
            .child(crate::SizedBox::new())
            .build();
        assert!(!widget.visible);
        assert!(widget.maintain_size);
        assert!(widget.maintain_state);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_visibility_macro_default() {
        let widget = visibility!();
        assert!(widget.visible);
    }

    #[test]
    fn test_visibility_macro_with_value() {
        let widget = visibility!(visible: false);
        assert!(!widget.visible);
    }
}
