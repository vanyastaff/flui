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
use flui_core::{DynRenderObject, DynWidget, MultiChildRenderObjectWidget, MultiChildRenderObjectElement, RenderObjectWidget, Widget};
use flui_rendering::{RenderStack, StackFit};
use flui_types::layout::Alignment;

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
/// - `StackFit::PassThrough` - Use incoming constraints as-is
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
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn = build_stack
)]
pub struct Stack {
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
    /// - `StackFit::PassThrough` - Use incoming constraints as-is
    #[builder(default = StackFit::Loose)]
    pub fit: StackFit,

    /// The child widgets.
    ///
    /// Children are painted in order, with the first child at the bottom
    /// and subsequent children painted on top.
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Vec<Box<dyn DynWidget>>,
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
            children: Vec::new(),
        }
    }

    /// Adds a child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut stack = Stack::new();
    /// stack.add_child(Container::new());
    /// stack.add_child(Text::new("Overlay"));
    /// ```
    pub fn add_child<W: Widget + 'static>(&mut self, child: W) {
        self.children.push(Box::new(child));
    }

    /// Sets the children widgets.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut stack = Stack::new();
    /// stack.set_children(vec![
    ///     Container::new(),
    ///     Text::new("Overlay"),
    /// ]);
    /// ```
    pub fn set_children(&mut self, children: Vec<Box<dyn Widget>>) {
        self.children = children;
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

// Implement Widget trait with associated type
impl Widget for Stack {
    type Element = MultiChildRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        MultiChildRenderObjectElement::new(self)
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for Stack {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        let mut render_stack = RenderStack::new();
        render_stack.set_alignment(self.alignment);
        render_stack.set_fit(self.fit);
        Box::new(render_stack)
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        if let Some(stack_render) = render_object.downcast_mut::<RenderStack>() {
            stack_render.set_alignment(self.alignment);
            stack_render.set_fit(self.fit);
        }
    }
}

// Implement MultiChildRenderObjectWidget
impl MultiChildRenderObjectWidget for Stack {
    fn children(&self) -> &[Box<dyn DynWidget>] {
        &self.children
    }
}

// bon Builder Extensions
use stack_builder::{IsUnset, SetChildren, State};

// Custom setter for children
impl<S: State> StackBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets the children widgets (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Stack::builder()
    ///     .alignment(Alignment::CENTER)
    ///     .children(vec![widget1, widget2])
    ///     .build()
    /// ```
    pub fn children(
        self,
        children: impl IntoIterator<Item = impl Widget + 'static>,
    ) -> StackBuilder<SetChildren<S>> {
        let boxed: Vec<Box<dyn DynWidget>> = children
            .into_iter()
            .map(|w| Box::new(w) as Box<dyn DynWidget>)
            .collect();
        self.children_internal(boxed)
    }
}

// Public build() wrapper
impl<S: State> StackBuilder<S> {
    /// Builds the Stack widget.
    ///
    /// Equivalent to calling the generated `build_stack()` finishing function.
    pub fn build(self) -> Stack {
        self.build_stack()
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
    use flui_core::{LeafRenderObjectElement, RenderObjectWidget};
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockWidget {
        #[allow(dead_code)]
        id: String,
    }

    impl MockWidget {
        fn new(id: &str) -> Self {
            Self { id: id.to_string() }
        }
    }

    impl Widget for MockWidget {
        type Element = LeafRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            LeafRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

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
    fn test_stack_add_child() {
        let mut widget = Stack::new();
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));
        assert_eq!(widget.children.len(), 2);
    }

    #[test]
    fn test_stack_set_children() {
        let mut widget = Stack::new();
        widget.set_children(vec![
            Box::new(MockWidget::new("child1")),
            Box::new(MockWidget::new("child2")),
            Box::new(MockWidget::new("child3")),
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
            .children(vec![MockWidget::new("child1"), MockWidget::new("child2")])
            .build();

        assert_eq!(widget.children.len(), 2);
    }

    #[test]
    fn test_stack_struct_literal() {
        let widget = Stack {
            alignment: Alignment::BOTTOM_RIGHT,
            fit: StackFit::PassThrough,
            ..Default::default()
        };

        assert_eq!(widget.alignment, Alignment::BOTTOM_RIGHT);
        assert_eq!(widget.fit, StackFit::PassThrough);
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
        let fits = [StackFit::Loose, StackFit::Expand, StackFit::PassThrough];

        for fit in fits {
            let widget = Stack::builder().fit(fit).build();
            assert_eq!(widget.fit, fit);
        }
    }

    #[test]
    fn test_stack_render_object_creation() {
        let widget = Stack::new();
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderStack>().is_some());
    }

    #[test]
    fn test_stack_render_object_update() {
        let widget1 = Stack::new();
        let mut render_object = widget1.create_render_object();

        let widget2 = Stack::builder()
            .alignment(Alignment::CENTER)
            .fit(StackFit::Expand)
            .build();
        widget2.update_render_object(&mut *render_object);

        // RenderStack doesn't expose getters, so we just verify it doesn't panic
    }

    #[test]
    fn test_stack_children_method() {
        let widget = Stack::new();
        assert_eq!(widget.children().len(), 0);

        let mut widget = Stack::new();
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));
        assert_eq!(widget.children().len(), 2);
    }

    #[test]
    fn test_stack_empty_children() {
        let widget = Stack::new();
        assert_eq!(widget.children.len(), 0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_stack_many_children() {
        let mut widget = Stack::new();
        for i in 0..10 {
            widget.add_child(MockWidget::new(&format!("child{}", i)));
        }
        assert_eq!(widget.children.len(), 10);
    }

    #[test]
    fn test_stack_widget_trait() {
        let widget = Stack::builder()
            .children(vec![MockWidget::new("1"), MockWidget::new("2")])
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_stack_multi_child() {
        let widget = Stack::builder()
            .alignment(Alignment::CENTER)
            .children(vec![
                MockWidget::new("background"),
                MockWidget::new("middle"),
                MockWidget::new("foreground"),
            ])
            .build();

        assert_eq!(widget.children.len(), 3);
        assert_eq!(widget.alignment, Alignment::CENTER);
    }
}
