//! Row widget - horizontal flex layout
//!
//! A widget that displays its children in a horizontal array.
//! Similar to Flutter's Row widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Row {
//!     main_axis_alignment: Some(MainAxisAlignment::Center),
//!     children: vec![child1, child2],
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Row::builder()
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .cross_axis_alignment(CrossAxisAlignment::Start)
//!     .children(vec![child1, child2])
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! row! {
//!     main_axis_alignment: MainAxisAlignment::Center,
//!     children: vec![child1, child2],
//! }
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, MultiChildRenderObjectWidget, RenderObjectWidget, Widget};
use flui_rendering::{MultiArity, RenderFlex};
use flui_types::{Axis, CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

/// A widget that displays its children in a horizontal array.
///
/// Row is a flex container that lays out its children horizontally (along the x-axis).
/// The children are positioned according to the main axis and cross axis alignment.
///
/// ## Layout Behavior
///
/// - **Main axis** (horizontal): Children are laid out left-to-right
/// - **Cross axis** (vertical): Children alignment depends on cross_axis_alignment
/// - **Main axis size**: Can be `Max` (fill available width) or `Min` (shrink to children)
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple row with centered children
/// Row::builder()
///     .main_axis_alignment(MainAxisAlignment::Center)
///     .children(vec![
///         Box::new(Text::new("Hello")),
///         Box::new(Text::new("World")),
///     ])
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(MainAxisAlignment, into),
    on(CrossAxisAlignment, into),
    on(MainAxisSize, into),
    finish_fn = build_row
)]
pub struct Row {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How children should be placed along the main axis (horizontal).
    ///
    /// Defaults to MainAxisAlignment::Start if not specified.
    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    /// How children should be aligned along the cross axis (vertical).
    ///
    /// Defaults to CrossAxisAlignment::Center if not specified.
    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,

    /// How much space should be occupied in the main axis.
    ///
    /// - `MainAxisSize::Max`: Row takes all available horizontal space
    /// - `MainAxisSize::Min`: Row shrinks to fit children
    ///
    /// Defaults to MainAxisSize::Max.
    #[builder(default = MainAxisSize::Max)]
    pub main_axis_size: MainAxisSize,

    /// The widgets to display in this row.
    ///
    /// Children are laid out horizontally in the order they appear in the vector.
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Vec<BoxedWidget>,
}

impl Row {
    /// Creates a new empty Row with default values.
    pub fn new() -> Self {
        Self {
            key: None,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
            children: Vec::new(),
        }
    }

    /// Adds a child widget to the row.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut row = Row::new();
    /// row.add_child(Text::new("Hello"));
    /// row.add_child(Text::new("World"));
    /// ```
    pub fn add_child<W: Widget + 'static>(&mut self, child: W) {
        self.children.push(BoxedWidget::new(child));
    }

    /// Sets all children at once.
    pub fn set_children(&mut self, children: Vec<BoxedWidget>) {
        self.children = children;
    }

    /// Validates row configuration.
    pub fn validate(&self) -> Result<(), String> {
        // No specific validation needed for Row
        // All enum values are already valid
        Ok(())
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Widget trait with associated type
impl Widget for Row {}

// Implement RenderObjectWidget
impl RenderObjectWidget for Row {
    type RenderObject = RenderFlex;
    type Arity = MultiArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderFlex::row()
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.main_axis_alignment = self.main_axis_alignment;
        render_object.cross_axis_alignment = self.cross_axis_alignment;
        render_object.main_axis_size = self.main_axis_size;
    }
}

impl MultiChildRenderObjectWidget for Row {
    fn children(&self) -> &[BoxedWidget] {
        &self.children
    }
}

// bon Builder Extensions
use row_builder::{IsUnset, SetChildren, State};

// Custom children setter
impl<S: State> RowBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets the children widgets (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Row::builder()
    ///     .children(vec![
    ///         Box::new(widget1),
    ///         Box::new(widget2),
    ///     ])
    ///     .build()
    /// ```
    pub fn children(self, children: Vec<BoxedWidget>) -> RowBuilder<SetChildren<S>> {
        self.children_internal(children)
    }
}

// Build wrapper - available for all states
impl<S: State> RowBuilder<S> {
    /// Builds the Row widget.
    pub fn build(self) -> Row {
        self.build_row()
    }
}

/// Macro for creating Row with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// row! {
///     main_axis_alignment: MainAxisAlignment::Center,
/// }
/// ```
#[macro_export]
macro_rules! row {
    () => {
        $crate::Row::new()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Row {
            $($field: $value,)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::LeafRenderObjectElement;
    use flui_rendering::RenderPadding;
    use flui_types::EdgeInsets;

    // Mock widget for testing
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
    fn test_row_new() {
        let row = Row::new();
        assert!(row.key.is_none());
        assert_eq!(row.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(row.cross_axis_alignment, CrossAxisAlignment::Center);
        assert_eq!(row.main_axis_size, MainAxisSize::Max);
        assert_eq!(row.children.len(), 0);
    }

    #[test]
    fn test_row_default() {
        let row = Row::default();
        assert_eq!(row.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(row.children.len(), 0);
    }

    #[test]
    fn test_row_struct_literal() {
        let row = Row {
            main_axis_alignment: MainAxisAlignment::Center,
            children: vec![Box::new(MockWidget)],
            ..Default::default()
        };
        assert_eq!(row.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(row.children.len(), 1);
    }

    #[test]
    fn test_row_builder() {
        let row = Row::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert_eq!(row.main_axis_alignment, MainAxisAlignment::Center);
    }

    #[test]
    fn test_row_builder_chaining() {
        let row = Row::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min)
            .build();

        assert_eq!(row.main_axis_alignment, MainAxisAlignment::SpaceBetween);
        assert_eq!(row.cross_axis_alignment, CrossAxisAlignment::Start);
        assert_eq!(row.main_axis_size, MainAxisSize::Min);
    }

    #[test]
    fn test_row_builder_children() {
        let row = Row::builder()
            .children(vec![
                Box::new(MockWidget) as Box<dyn DynWidget>,
                Box::new(MockWidget) as Box<dyn DynWidget>,
            ])
            .build();

        assert_eq!(row.children.len(), 2);
    }

    #[test]
    fn test_row_add_child() {
        let mut row = Row::new();
        row.add_child(MockWidget);
        row.add_child(MockWidget);
        assert_eq!(row.children.len(), 2);
    }

    #[test]
    fn test_row_set_children() {
        let mut row = Row::new();
        row.set_children(vec![
            Box::new(MockWidget) as Box<dyn DynWidget>,
            Box::new(MockWidget) as Box<dyn DynWidget>,
        ]);
        assert_eq!(row.children.len(), 2);
    }

    #[test]
    fn test_row_macro_empty() {
        let row = row!();
        assert_eq!(row.children.len(), 0);
    }

    #[test]
    fn test_row_macro_with_fields() {
        let row = row! {
            main_axis_alignment: MainAxisAlignment::End,
            cross_axis_alignment: CrossAxisAlignment::Stretch,
        };
        assert_eq!(row.main_axis_alignment, MainAxisAlignment::End);
        assert_eq!(row.cross_axis_alignment, CrossAxisAlignment::Stretch);
    }

    #[test]
    fn test_row_validate_ok() {
        let row = Row::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert!(row.validate().is_ok());
    }

    #[test]
    fn test_row_all_main_axis_alignments() {
        for alignment in [
            MainAxisAlignment::Start,
            MainAxisAlignment::End,
            MainAxisAlignment::Center,
            MainAxisAlignment::SpaceBetween,
            MainAxisAlignment::SpaceAround,
            MainAxisAlignment::SpaceEvenly,
        ] {
            let row = Row::builder().main_axis_alignment(alignment).build();
            assert_eq!(row.main_axis_alignment, alignment);
        }
    }

    #[test]
    fn test_row_all_cross_axis_alignments() {
        for alignment in [
            CrossAxisAlignment::Start,
            CrossAxisAlignment::End,
            CrossAxisAlignment::Center,
            CrossAxisAlignment::Stretch,
        ] {
            let row = Row::builder().cross_axis_alignment(alignment).build();
            assert_eq!(row.cross_axis_alignment, alignment);
        }
    }

    #[test]
    fn test_row_widget_trait() {
        let row = Row::builder()
            .children(vec![Box::new(MockWidget), Box::new(MockWidget)])
            .build();

        // Test that it implements Widget and can create an element
        let _element = row.into_element();
    }

    #[test]
    fn test_row_multi_child() {
        let row = Row::builder()
            .children(vec![
                Box::new(MockWidget) as Box<dyn DynWidget>,
                Box::new(MockWidget) as Box<dyn DynWidget>,
                Box::new(MockWidget) as Box<dyn DynWidget>,
            ])
            .build();

        assert_eq!(row.children.len(), 3);
    }
}
