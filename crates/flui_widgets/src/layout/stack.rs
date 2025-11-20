//! Stack widget - overlays children on top of each other
//!
//! A widget that positions its children relative to the edges of its box.
//! Similar to Flutter's Stack widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Stack {
//!     alignment: Alignment::CENTER,
//!     children: vec![widget1, widget2],
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Stack::builder()
//!     .alignment(Alignment::TOP_LEFT)
//!     .children(vec![widget1, widget2])
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! stack! {
//!     alignment: Alignment::CENTER,
//! }
//! ```

use bon::Builder;
use flui_core::render::RenderBoxExt;
use flui_core::view::children::Children;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderStack;
use flui_types::layout::{Alignment, StackFit};

/// A widget that positions its children relative to the edges of its box.
///
/// Stack allows children to overlap. The first child is painted at the bottom,
/// and each subsequent child is painted on top.
///
/// ## Layout Behavior
///
/// Stack lays out children in two groups:
///
/// 1. **Non-positioned children**: Laid out with loose or tight constraints,
///    then positioned according to the stack's alignment.
///
/// 2. **Positioned children**: Wrapped in Positioned widget, which specifies
///    their position using left/top/right/bottom coordinates.
///
/// The stack's size is determined by:
/// - `StackFit::Loose` - Size to fit non-positioned children
/// - `StackFit::Expand` - Expand to fill incoming constraints
/// - `StackFit::Passthrough` - Use incoming constraints as-is
///
/// ## Common Use Cases
///
/// ### Simple Overlay
/// ```rust,ignore
/// Stack::new()
///     .children(vec![
///         Container::new().width(200.0).height(200.0).color(Color::BLUE),
///         Container::new().width(100.0).height(100.0).color(Color::RED),
///     ])
/// ```
///
/// ### Positioned Children
/// ```rust,ignore
/// Stack::new()
///     .children(vec![
///         // Background
///         Container::new().width(300.0).height(300.0),
///         // Top-left corner
///         Positioned::new()
///             .left(10.0)
///             .top(10.0)
///             .child(Text::new("Top Left")),
///         // Bottom-right corner
///         Positioned::new()
///             .right(10.0)
///             .bottom(10.0)
///             .child(Text::new("Bottom Right")),
///     ])
/// ```
///
/// ### Centered Overlay
/// ```rust,ignore
/// Stack::builder()
///     .alignment(Alignment::CENTER)
///     .children(vec![
///         Image::asset("background.png"),
///         CircularProgressIndicator::new(),
///     ])
///     .build()
/// ```
///
/// ## Performance Considerations
///
/// - Children are painted in order (first child is at the bottom)
/// - Positioned children can overflow the stack's bounds
/// - Use `StackFit::Expand` to ensure the stack fills available space
///
/// ## Examples
///
/// ```rust,ignore
/// // Card with floating action button
/// Stack::builder()
///     .alignment(Alignment::BOTTOM_RIGHT)
///     .children(vec![
///         Container::new()
///             .width(300.0)
///             .height(200.0)
///             .color(Color::WHITE),
///         Positioned::new()
///             .right(16.0)
///             .bottom(-28.0)
///             .child(FloatingActionButton::new()),
///     ])
///     .build()
///
/// // Avatar with badge
/// Stack::new()
///     .children(vec![
///         CircleAvatar::new(radius: 40.0),
///         Positioned::new()
///             .right(0.0)
///             .top(0.0)
///             .child(Badge::new("3")),
///     ])
/// ```
///
/// ## See Also
///
/// - Positioned: For positioning children within Stack
/// - IndexedStack: Shows only one child at a time
/// - Align: For simple alignment without overlapping
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Stack {
    /// The child widgets.
    ///
    /// Children are painted in order, with the first child at the bottom
    /// and subsequent children painted on top.
    ///
    /// Can be set via:
    /// - `.children(vec![...])` to set all at once
    /// - `.child(widget)` repeatedly to add one at a time (chainable)
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Children,

    /// Optional key for widget identification
    pub key: Option<String>,

    /// How to align non-positioned children.
    ///
    /// Children that are not wrapped in Positioned will be aligned
    /// according to this alignment within the stack's bounds.
    ///
    /// Common values:
    /// - `Alignment::TOP_LEFT` (default)
    /// - `Alignment::CENTER`
    /// - `Alignment::BOTTOM_RIGHT`
    #[builder(default = Alignment::TOP_LEFT)]
    pub alignment: Alignment,

    /// How to size the stack.
    ///
    /// - `StackFit::Loose` - Size to fit non-positioned children (default)
    /// - `StackFit::Expand` - Expand to fill incoming constraints
    /// - `StackFit::Passthrough` - Use incoming constraints as-is
    #[builder(default = StackFit::Loose)]
    pub fit: StackFit,
}

impl std::fmt::Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stack")
            .field("key", &self.key)
            .field("alignment", &self.alignment)
            .field("fit", &self.fit)
            .field(
                "children",
                &if !self.children.is_empty() {
                    "<>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Stack {
    /// Creates a new Stack widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Stack::new();
    /// ```
    pub fn new() -> Self {
        Self {
            key: None,
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::Loose,
            children: Children::default(),
        }
    }

    /// Adds a child widget.
    #[deprecated(note = "Use builder pattern with chainable .child() instead")]
    pub fn add_child(&mut self, child: impl View + 'static) {
        self.children.push(child);
    }

    /// Sets the children widgets.
    #[deprecated(note = "Use builder pattern with .children() instead")]
    pub fn set_children(&mut self, children: impl Into<Children>) {
        self.children = children.into();
    }

    /// Validates Stack configuration.
    ///
    /// Currently always returns Ok, but may add validation in the future.
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View for Stack - New architecture
impl View for Stack {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut render_stack = RenderStack::with_alignment(self.alignment);
        render_stack.fit = self.fit;

        render_stack.children(self.children.into_inner())
    }
}

// bon Builder Extensions - Custom builder methods for StackBuilder
use stack_builder::{IsUnset, SetChildren, State};

impl<S: State> StackBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets all children at once.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Stack::builder()
    ///     .alignment(Alignment::CENTER)
    ///     .children(vec![
    ///         Box::new(widget1),
    ///         Box::new(widget2),
    ///     ])
    ///     .build()
    /// ```
    pub fn children(self, children: impl Into<Children>) -> StackBuilder<SetChildren<S>> {
        self.children_internal(children.into())
    }
}

impl<S: State> StackBuilder<S> {
    /// Builds the Stack with optional validation.
    pub fn build(self) -> Stack {
        let stack = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = stack.validate() {
                tracing::warn!("Stack validation failed: {}", e);
            }
        }

        stack
    }
}

/// Macro for creating Stack with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Empty stack
/// stack!()
///
/// // With alignment
/// stack! {
///     alignment: Alignment::CENTER,
/// }
///
/// // With fit
/// stack! {
///     fit: StackFit::Expand,
/// }
/// ```
#[macro_export]
macro_rules! stack {
    () => {
        $crate::Stack::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Stack {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock view for testing
    #[derive()]
    struct MockView {
        #[allow(dead_code)]
        id: String,
    }

    impl MockView {
        fn new(id: &str) -> Self {
            Self { id: id.to_string() }
        }
    }

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            (RenderPadding::new(EdgeInsets::ZERO), ())
        }
    }

    #[test]
    fn test_stack_new() {
        let widget = Stack::new();
        assert!(widget.key.is_none());
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert_eq!(widget.fit, StackFit::Loose);
        assert_eq!(widget.children.len(), 0);
    }

    #[test]
    fn test_stack_default() {
        let widget = Stack::default();
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert_eq!(widget.fit, StackFit::Loose);
    }

    #[test]
    #[allow(deprecated)]
    fn test_stack_add_child() {
        let mut widget = Stack::new();
        widget.child(MockView::new("child1"));
        widget.child(MockView::new("child2"));
        assert_eq!(widget.children.len(), 2);
    }

    #[test]
    #[allow(deprecated)]
    fn test_stack_set_children() {
        let mut widget = Stack::new();
        widget.set_children(vec![
            Box::new(MockView::new("child1")),
            Box::new(MockView::new("child2")),
            Box::new(MockView::new("child3")),
        ]);
        assert_eq!(widget.children.len(), 3);
    }

    #[test]
    fn test_stack_builder() {
        let widget = Stack::builder()
            .alignment(Alignment::CENTER)
            .fit(StackFit::Expand)
            .build();

        assert_eq!(widget.alignment, Alignment::CENTER);
        assert_eq!(widget.fit, StackFit::Expand);
    }

    #[test]
    fn test_stack_builder_with_children() {
        let widget = Stack::builder()
            .children(vec![
                Box::new(MockView::new("child1")),
                Box::new(MockView::new("child2")),
            ])
            .build();

        assert_eq!(widget.children.len(), 2);
    }

    #[test]
    fn test_stack_struct_literal() {
        let widget = Stack {
            alignment: Alignment::BOTTOM_RIGHT,
            fit: StackFit::Passthrough,
            ..Default::default()
        };

        assert_eq!(widget.alignment, Alignment::BOTTOM_RIGHT);
        assert_eq!(widget.fit, StackFit::Passthrough);
    }

    #[test]
    fn test_stack_macro_empty() {
        let widget = stack!();
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_stack_macro_with_alignment() {
        let widget = stack! {
            alignment: Alignment::CENTER,
        };
        assert_eq!(widget.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_stack_macro_with_fit() {
        let widget = stack! {
            fit: StackFit::Expand,
        };
        assert_eq!(widget.fit, StackFit::Expand);
    }

    #[test]
    fn test_stack_validate_ok() {
        let widget = Stack::new();
        assert!(widget.validate().is_ok());

        let widget = Stack::builder()
            .alignment(Alignment::CENTER)
            .fit(StackFit::Expand)
            .build();
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_stack_all_alignments() {
        let alignments = [
            Alignment::TOP_LEFT,
            Alignment::TOP_CENTER,
            Alignment::TOP_RIGHT,
            Alignment::CENTER_LEFT,
            Alignment::CENTER,
            Alignment::CENTER_RIGHT,
            Alignment::BOTTOM_LEFT,
            Alignment::BOTTOM_CENTER,
            Alignment::BOTTOM_RIGHT,
        ];

        for alignment in alignments {
            let widget = Stack::builder().alignment(alignment).build();
            assert_eq!(widget.alignment, alignment);
        }
    }

    #[test]
    fn test_stack_all_fits() {
        let fits = [StackFit::Loose, StackFit::Expand, StackFit::Passthrough];

        for fit in fits {
            let widget = Stack::builder().fit(fit).build();
            assert_eq!(widget.fit, fit);
        }
    }

    #[test]
    fn test_stack_empty_children() {
        let widget = Stack::new();
        assert_eq!(widget.children.len(), 0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    #[allow(deprecated)]
    fn test_stack_many_children() {
        let mut widget = Stack::new();
        for i in 0..10 {
            widget.child(MockView::new(&format!("child{}", i)));
        }
        assert_eq!(widget.children.len(), 10);
    }

    #[test]
    fn test_stack_multi_child() {
        let widget = Stack::builder()
            .alignment(Alignment::CENTER)
            .children(vec![
                Box::new(MockView::new("background")),
                Box::new(MockView::new("middle")),
                Box::new(MockView::new("foreground")),
            ])
            .build();

        assert_eq!(widget.children.len(), 3);
        assert_eq!(widget.alignment, Alignment::CENTER);
    }
}

// Stack now implements View trait directly
