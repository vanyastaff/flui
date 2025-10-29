//! IndexedStack widget - shows only one child by index
//!
//! A widget that shows only a single child from a list of children.
//! Similar to Flutter's IndexedStack widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! IndexedStack {
//!     index: Some(0),
//!     children: vec![widget1, widget2],
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! IndexedStack::builder()
//!     .index(1)
//!     .children(vec![widget1, widget2])
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! indexed_stack! {
//!     index: 0,
//! }
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, DynWidget, MultiChildRenderObjectWidget, RenderObjectWidget, Widget};
use flui_rendering::{MultiArity, RenderIndexedStack};
use flui_types::layout::{Alignment, StackFit};

/// A widget that shows only a single child from a list of children.
///
/// Unlike Stack which shows all children overlaid, IndexedStack shows only the
/// child at the specified index. However, all children are laid out (but hidden)
/// to maintain their state and compute the correct size.
///
/// ## Layout Behavior
///
/// - All children are laid out with the same constraints
/// - Stack size is the maximum of all children's sizes (or fills constraints if sizing is Expand)
/// - Only the child at `index` is painted
/// - If index is None or out of bounds, no child is painted
///
/// ## Common Use Cases
///
/// ### Tab Navigation
/// ```rust,ignore
/// let current_tab = 0;
/// IndexedStack::builder()
///     .index(current_tab)
///     .children(vec![
///         HomeTab::new(),
///         SearchTab::new(),
///         ProfileTab::new(),
///     ])
///     .build()
/// ```
///
/// ### Wizard Steps
/// ```rust,ignore
/// let current_step = 1;
/// IndexedStack::builder()
///     .index(current_step)
///     .sizing(StackFit::Expand)
///     .children(vec![
///         WizardStep1::new(),
///         WizardStep2::new(),
///         WizardStep3::new(),
///     ])
///     .build()
/// ```
///
/// ### Page Views
/// ```rust,ignore
/// IndexedStack::builder()
///     .index(page_index)
///     .alignment(Alignment::CENTER)
///     .children(vec![
///         Page1::new(),
///         Page2::new(),
///         Page3::new(),
///     ])
///     .build()
/// ```
///
/// ## Performance Considerations
///
/// - All children are laid out even if not visible
/// - Useful when you want to maintain child state between switches
/// - For better performance with many children, consider lazy loading
/// - Children not at current index still consume layout resources
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple tab switcher
/// let mut current_tab = 0;
///
/// let tabs = IndexedStack::builder()
///     .index(current_tab)
///     .children(vec![
///         Container::new().color(Color::RED).width(100.0).height(100.0),
///         Container::new().color(Color::GREEN).width(150.0).height(150.0),
///         Container::new().color(Color::BLUE).width(200.0).height(200.0),
///     ])
///     .build();
///
/// // Change tab
/// current_tab = 1;
/// ```
///
/// ## See Also
///
/// - Stack: For showing all children overlaid
/// - PageView: For swiping between pages
/// - TabBarView: For tab-based navigation with animations
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn = build_indexed_stack
)]
pub struct IndexedStack {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Index of the child to show.
    ///
    /// If None or out of bounds, no child is painted.
    /// All children are still laid out regardless.
    pub index: Option<usize>,

    /// How to align children within the stack.
    ///
    /// When a child is smaller than the stack, this alignment
    /// determines where it appears.
    #[builder(default = Alignment::TOP_LEFT)]
    pub alignment: Alignment,

    /// How to size the stack.
    ///
    /// - `StackFit::Loose` - Size to fit children (default)
    /// - `StackFit::Expand` - Expand to fill incoming constraints
    /// - `StackFit::Passthrough` - Use incoming constraints as-is
    #[builder(default = StackFit::Loose)]
    pub sizing: StackFit,

    /// The child widgets.
    ///
    /// Only the child at `index` will be visible, but all children
    /// are laid out to compute the correct size and maintain state.
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Vec<BoxedWidget>,
}

impl IndexedStack {
    /// Creates a new IndexedStack widget.
    ///
    /// # Arguments
    ///
    /// * `index` - Index of child to show (None = show nothing)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IndexedStack::new(Some(0));
    /// ```
    pub fn new(index: Option<usize>) -> Self {
        Self {
            key: None,
            index,
            alignment: Alignment::TOP_LEFT,
            sizing: StackFit::Loose,
            children: Vec::new(),
        }
    }

    /// Creates an IndexedStack that expands to fill available space.
    ///
    /// # Arguments
    ///
    /// * `index` - Index of child to show
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IndexedStack::expand(Some(1));
    /// ```
    pub fn expand(index: Option<usize>) -> Self {
        Self {
            key: None,
            index,
            alignment: Alignment::TOP_LEFT,
            sizing: StackFit::Expand,
            children: Vec::new(),
        }
    }

    /// Adds a child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut stack = IndexedStack::new(Some(0));
    /// stack.add_child(Container::new());
    /// stack.add_child(Text::new("Page 2"));
    /// ```
    pub fn add_child<W: Widget + 'static>(&mut self, child: W) {
        self.children.push(BoxedWidget::new(child));
    }

    /// Sets the children widgets.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut stack = IndexedStack::new(Some(0));
    /// stack.set_children(vec![
    ///     Container::new(),
    ///     Text::new("Page 2"),
    /// ]);
    /// ```
    pub fn set_children(&mut self, children: Vec<Box<dyn DynWidget>>) {
        self.children = children;
    }

    /// Validates IndexedStack configuration.
    ///
    /// Returns an error if the index is out of bounds (when Some).
    pub fn validate(&self) -> Result<(), String> {
        if let Some(idx) = self.index {
            if idx >= self.children.len() && !self.children.is_empty() {
                return Err(format!(
                    "Index {} is out of bounds for {} children",
                    idx,
                    self.children.len()
                ));
            }
        }
        Ok(())
    }
}

impl Default for IndexedStack {
    fn default() -> Self {
        Self::new(None)
    }
}

// Implement Widget trait with associated type
impl Widget for IndexedStack {}

// Implement RenderObjectWidget
impl RenderObjectWidget for IndexedStack {
    type RenderObject = RenderIndexedStack;
    type Arity = MultiArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderIndexedStack::with_alignment(self.index, self.alignment)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_index(self.index);
        render_object.set_alignment(self.alignment);
    }
}

impl MultiChildRenderObjectWidget for IndexedStack {
    fn children(&self) -> &[BoxedWidget] {
        &self.children
    }
}

// bon Builder Extensions
use indexed_stack_builder::{IsUnset, SetChildren, State};

// Custom setter for children
impl<S: State> IndexedStackBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets the children widgets (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// IndexedStack::builder()
    ///     .index(0)
    ///     .children(vec![widget1, widget2])
    ///     .build()
    /// ```
    pub fn children(
        self,
        children: impl IntoIterator<Item = impl Widget + 'static>,
    ) -> IndexedStackBuilder<SetChildren<S>> {
        let boxed: Vec<Box<dyn DynWidget>> = children
            .into_iter()
            .map(|w| Box::new(w) as Box<dyn DynWidget>)
            .collect();
        self.children_internal(boxed)
    }
}

// Public build() wrapper
impl<S: State> IndexedStackBuilder<S> {
    /// Builds the IndexedStack widget.
    ///
    /// Equivalent to calling the generated `build_indexed_stack()` finishing function.
    pub fn build(self) -> IndexedStack {
        self.build_indexed_stack()
    }
}

/// Macro for creating IndexedStack with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Show first child
/// indexed_stack! {
///     index: 0,
/// }
///
/// // With alignment
/// indexed_stack! {
///     index: 1,
///     alignment: Alignment::CENTER,
/// }
///
/// // With sizing
/// indexed_stack! {
///     index: 2,
///     sizing: StackFit::Expand,
/// }
/// ```
#[macro_export]
macro_rules! indexed_stack {
    () => {
        $crate::IndexedStack::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::IndexedStack {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::{LeafRenderObjectElement, RenderObjectWidget};
    use flui_rendering::RenderPadding;
    use flui_types::EdgeInsets;

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

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

    #[test]
    fn test_indexed_stack_new() {
        let widget = IndexedStack::new(Some(0));
        assert!(widget.key.is_none());
        assert_eq!(widget.index, Some(0));
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert_eq!(widget.sizing, StackFit::Loose);
        assert_eq!(widget.children.len(), 0);
    }

    #[test]
    fn test_indexed_stack_new_none() {
        let widget = IndexedStack::new(None);
        assert_eq!(widget.index, None);
    }

    #[test]
    fn test_indexed_stack_default() {
        let widget = IndexedStack::default();
        assert_eq!(widget.index, None);
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert_eq!(widget.sizing, StackFit::Loose);
    }

    #[test]
    fn test_indexed_stack_expand() {
        let widget = IndexedStack::expand(Some(1));
        assert_eq!(widget.index, Some(1));
        assert_eq!(widget.sizing, StackFit::Expand);
    }

    #[test]
    fn test_indexed_stack_add_child() {
        let mut widget = IndexedStack::new(Some(0));
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));
        assert_eq!(widget.children.len(), 2);
    }

    #[test]
    fn test_indexed_stack_set_children() {
        let mut widget = IndexedStack::new(Some(0));
        widget.set_children(vec![
            Box::new(MockWidget::new("child1")),
            Box::new(MockWidget::new("child2")),
            Box::new(MockWidget::new("child3")),
        ]);
        assert_eq!(widget.children.len(), 3);
    }

    #[test]
    fn test_indexed_stack_builder() {
        let widget = IndexedStack::builder()
            .index(1)
            .alignment(Alignment::CENTER)
            .sizing(StackFit::Expand)
            .build();

        assert_eq!(widget.index, Some(1));
        assert_eq!(widget.alignment, Alignment::CENTER);
        assert_eq!(widget.sizing, StackFit::Expand);
    }

    #[test]
    fn test_indexed_stack_builder_with_children() {
        let widget = IndexedStack::builder()
            .index(0)
            .children(vec![MockWidget::new("child1"), MockWidget::new("child2")])
            .build();

        assert_eq!(widget.children.len(), 2);
    }

    #[test]
    fn test_indexed_stack_struct_literal() {
        let widget = IndexedStack {
            index: Some(2),
            alignment: Alignment::BOTTOM_RIGHT,
            sizing: StackFit::Passthrough,
            ..Default::default()
        };

        assert_eq!(widget.index, Some(2));
        assert_eq!(widget.alignment, Alignment::BOTTOM_RIGHT);
        assert_eq!(widget.sizing, StackFit::Passthrough);
    }

    #[test]
    fn test_indexed_stack_macro_empty() {
        let widget = indexed_stack!();
        assert_eq!(widget.index, None);
    }

    #[test]
    fn test_indexed_stack_macro_with_index() {
        let widget = indexed_stack! {
            index: Some(1),
        };
        assert_eq!(widget.index, Some(1));
    }

    #[test]
    fn test_indexed_stack_macro_with_fields() {
        let widget = indexed_stack! {
            index: Some(0),
            alignment: Alignment::CENTER,
        };
        assert_eq!(widget.index, Some(0));
        assert_eq!(widget.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_indexed_stack_validate_ok() {
        // Empty children with None index
        let widget = IndexedStack::new(None);
        assert!(widget.validate().is_ok());

        // Index within bounds
        let mut widget = IndexedStack::new(Some(1));
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));
        widget.add_child(MockWidget::new("child3"));
        assert!(widget.validate().is_ok());

        // Index 0 with children
        let mut widget = IndexedStack::new(Some(0));
        widget.add_child(MockWidget::new("child1"));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_indexed_stack_validate_out_of_bounds() {
        let mut widget = IndexedStack::new(Some(5));
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_indexed_stack_all_alignments() {
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
            let widget = IndexedStack::builder()
                .index(0)
                .alignment(alignment)
                .build();
            assert_eq!(widget.alignment, alignment);
        }
    }

    #[test]
    fn test_indexed_stack_all_sizings() {
        let sizings = [StackFit::Loose, StackFit::Expand, StackFit::Passthrough];

        for sizing in sizings {
            let widget = IndexedStack::builder().index(0).sizing(sizing).build();
            assert_eq!(widget.sizing, sizing);
        }
    }

    #[test]
    fn test_indexed_stack_render_object_creation() {
        let widget = IndexedStack::new(Some(0));
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderIndexedStack>().is_some());
    }

    #[test]
    fn test_indexed_stack_render_object_update() {
        let widget1 = IndexedStack::new(Some(0));
        let mut render_object = widget1.create_render_object();

        let widget2 = IndexedStack::builder()
            .index(1)
            .alignment(Alignment::CENTER)
            .sizing(StackFit::Expand)
            .build();
        widget2.update_render_object(&mut *render_object);

        let indexed_stack_render = render_object.downcast_ref::<RenderIndexedStack>().unwrap();
        assert_eq!(indexed_stack_render.index(), Some(1));
    }

    #[test]
    fn test_indexed_stack_children_method() {
        let widget = IndexedStack::new(Some(0));
        assert_eq!(widget.children().len(), 0);

        let mut widget = IndexedStack::new(Some(0));
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));
        assert_eq!(widget.children().len(), 2);
    }

    #[test]
    fn test_indexed_stack_empty_children() {
        let widget = IndexedStack::new(None);
        assert_eq!(widget.children.len(), 0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_indexed_stack_many_children() {
        let mut widget = IndexedStack::new(Some(5));
        for i in 0..10 {
            widget.add_child(MockWidget::new(&format!("child{}", i)));
        }
        assert_eq!(widget.children.len(), 10);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_indexed_stack_index_change() {
        let mut widget = IndexedStack::new(Some(0));
        widget.add_child(MockWidget::new("child1"));
        widget.add_child(MockWidget::new("child2"));

        assert_eq!(widget.index, Some(0));
        assert!(widget.validate().is_ok());

        widget.index = Some(1);
        assert_eq!(widget.index, Some(1));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_indexed_stack_widget_trait() {
        let widget = IndexedStack::builder()
            .index(Some(0))
            .children(vec![MockWidget::new("1"), MockWidget::new("2")])
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_indexed_stack_multi_child() {
        let widget = IndexedStack::builder()
            .index(Some(1))
            .children(vec![
                MockWidget::new("page1"),
                MockWidget::new("page2"),
                MockWidget::new("page3"),
            ])
            .build();

        assert_eq!(widget.children.len(), 3);
        assert_eq!(widget.index, Some(1));
    }
}
