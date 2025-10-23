//! Column widget - vertical flex layout
//!
//! A widget that displays its children in a vertical array.
//! Similar to Flutter's Column widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Column {
//!     main_axis_alignment: Some(MainAxisAlignment::Center),
//!     children: vec![child1, child2],
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Column::builder()
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .cross_axis_alignment(CrossAxisAlignment::Start)
//!     .children(vec![child1, child2])
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! column! {
//!     main_axis_alignment: MainAxisAlignment::Center,
//!     children: vec![child1, child2],
//! }
//! ```

use bon::Builder;
use flui_core::{DynRenderObject, DynWidget, MultiChildRenderObjectWidget, MultiChildRenderObjectElement, RenderObjectWidget, Widget};
use flui_rendering::RenderFlex;
use flui_types::{Axis, CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

/// A widget that displays its children in a vertical array.
///
/// Column is a flex container that lays out its children vertically (along the y-axis).
/// The children are positioned according to the main axis and cross axis alignment.
///
/// ## Layout Behavior
///
/// - **Main axis** (vertical): Children are laid out top-to-bottom
/// - **Cross axis** (horizontal): Children alignment depends on cross_axis_alignment
/// - **Main axis size**: Can be `Max` (fill available height) or `Min` (shrink to children)
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple column with centered children
/// Column::builder()
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
    finish_fn = build_column
)]
pub struct Column {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How children should be placed along the main axis (vertical).
    ///
    /// Defaults to MainAxisAlignment::Start if not specified.
    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    /// How children should be aligned along the cross axis (horizontal).
    ///
    /// Defaults to CrossAxisAlignment::Center if not specified.
    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,

    /// How much space should be occupied in the main axis.
    ///
    /// - `MainAxisSize::Max`: Column takes all available vertical space
    /// - `MainAxisSize::Min`: Column shrinks to fit children
    ///
    /// Defaults to MainAxisSize::Max.
    #[builder(default = MainAxisSize::Max)]
    pub main_axis_size: MainAxisSize,

    /// The widgets to display in this column.
    ///
    /// Children are laid out vertically (top-to-bottom) in the order they appear in the vector.
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Vec<Box<dyn DynWidget>>,
}

impl Column {
    /// Creates a new empty Column with default values.
    pub fn new() -> Self {
        Self {
            key: None,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
            children: Vec::new(),
        }
    }

    /// Adds a child widget to the column.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut column = Column::new();
    /// column.add_child(Text::new("Hello"));
    /// column.add_child(Text::new("World"));
    /// ```
    pub fn add_child<W: Widget + 'static>(&mut self, child: W) {
        self.children.push(Box::new(child));
    }

    /// Sets all children at once.
    pub fn set_children(&mut self, children: Vec<Box<dyn DynWidget>>) {
        self.children = children;
    }

    /// Validates column configuration.
    pub fn validate(&self) -> Result<(), String> {
        // No specific validation needed for Column
        // All enum values are already valid
        Ok(())
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Widget trait with associated type
impl Widget for Column {
    type Element = MultiChildRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        MultiChildRenderObjectElement::new(self)
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for Column {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        let flex = RenderFlex::column()
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size);
        Box::new(flex)
    }

    fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {
        // RenderFlex is immutable data - updates are handled by recreating the RenderObject
        // TODO: Implement proper update strategy once architecture is finalized
    }
}

// Implement MultiChildRenderObjectWidget
impl MultiChildRenderObjectWidget for Column {
    fn children(&self) -> &[Box<dyn DynWidget>] {
        &self.children
    }
}

// bon Builder Extensions
use column_builder::{IsUnset, SetChildren, State};

// Custom children setter
impl<S: State> ColumnBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets the children widgets (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Column::builder()
    ///     .children(vec![
    ///         Box::new(widget1),
    ///         Box::new(widget2),
    ///     ])
    ///     .build()
    /// ```
    pub fn children(self, children: Vec<Box<dyn DynWidget>>) -> ColumnBuilder<SetChildren<S>> {
        self.children_internal(children)
    }
}

// Build wrapper - available for all states
impl<S: State> ColumnBuilder<S> {
    /// Builds the Column widget.
    pub fn build(self) -> Column {
        self.build_column()
    }
}

/// Macro for creating Column with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// column! {
///     main_axis_alignment: MainAxisAlignment::Center,
/// }
/// ```
#[macro_export]
macro_rules! column {
    () => {
        $crate::Column::new()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Column {
            $($field: $value,)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockWidget;

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
    fn test_column_new() {
        let column = Column::new();
        assert!(column.key.is_none());
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Center);
        assert_eq!(column.main_axis_size, MainAxisSize::Max);
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_default() {
        let column = Column::default();
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_struct_literal() {
        let column = Column {
            main_axis_alignment: MainAxisAlignment::Center,
            children: vec![Box::new(MockWidget)],
            ..Default::default()
        };
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(column.children.len(), 1);
    }

    #[test]
    fn test_column_builder() {
        let column = Column::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Center);
    }

    #[test]
    fn test_column_builder_chaining() {
        let column = Column::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min)
            .build();

        assert_eq!(column.main_axis_alignment, MainAxisAlignment::SpaceBetween);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Start);
        assert_eq!(column.main_axis_size, MainAxisSize::Min);
    }

    #[test]
    fn test_column_builder_children() {
        let column = Column::builder()
            .children(vec![
                Box::new(MockWidget) as Box<dyn DynWidget>,
                Box::new(MockWidget) as Box<dyn DynWidget>,
            ])
            .build();

        assert_eq!(column.children.len(), 2);
    }


    #[test]
    fn test_column_add_child() {
        let mut column = Column::new();
        column.add_child(MockWidget);
        column.add_child(MockWidget);
        assert_eq!(column.children.len(), 2);
    }

    #[test]
    fn test_column_set_children() {
        let mut column = Column::new();
        column.set_children(vec![
            Box::new(MockWidget) as Box<dyn DynWidget>,
            Box::new(MockWidget) as Box<dyn DynWidget>,
        ]);
        assert_eq!(column.children.len(), 2);
    }

    #[test]
    fn test_column_macro_empty() {
        let column = column!();
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_macro_with_fields() {
        let column = column! {
            main_axis_alignment: MainAxisAlignment::End,
            cross_axis_alignment: CrossAxisAlignment::Stretch,
        };
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::End);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Stretch);
    }

    #[test]
    fn test_column_validate_ok() {
        let column = Column::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert!(column.validate().is_ok());
    }

    #[test]
    fn test_column_all_main_axis_alignments() {
        for alignment in [
            MainAxisAlignment::Start,
            MainAxisAlignment::End,
            MainAxisAlignment::Center,
            MainAxisAlignment::SpaceBetween,
            MainAxisAlignment::SpaceAround,
            MainAxisAlignment::SpaceEvenly,
        ] {
            let column = Column::builder()
                .main_axis_alignment(alignment)
                .build();
            assert_eq!(column.main_axis_alignment, alignment);
        }
    }

    #[test]
    fn test_column_all_cross_axis_alignments() {
        for alignment in [
            CrossAxisAlignment::Start,
            CrossAxisAlignment::End,
            CrossAxisAlignment::Center,
            CrossAxisAlignment::Stretch,
        ] {
            let column = Column::builder()
                .cross_axis_alignment(alignment)
                .build();
            assert_eq!(column.cross_axis_alignment, alignment);
        }
    }

    #[test]
    fn test_column_widget_trait() {
        let column = Column::builder()
            .children(vec![Box::new(MockWidget), Box::new(MockWidget)])
            .build();

        // Test that it implements Widget and can create an element
        let _element = column.into_element();
    }

    #[test]
    fn test_column_multi_child() {
        let column = Column::builder()
            .children(vec![
                Box::new(MockWidget) as Box<dyn DynWidget>,
                Box::new(MockWidget) as Box<dyn DynWidget>,
                Box::new(MockWidget) as Box<dyn DynWidget>,
            ])
            .build();

        assert_eq!(column.children.len(), 3);
    }
}
