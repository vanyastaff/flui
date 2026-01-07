//! Flex layout widgets - Row and Column
//!
//! Widgets that arrange children in a horizontal row or vertical column.
//! Similar to Flutter's Row and Column widgets.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Horizontal row
//! Row::new()
//!     .spacing(8.0)
//!     .children([box1, box2, box3])
//!
//! // Vertical column
//! Column::new()
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .children([item1, item2])
//! ```

use flui_rendering::objects::{FlexDirection, RenderFlex};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{impl_render_view, Children, RenderView, View};

// Re-export alignment types for convenience
pub use flui_rendering::objects::{CrossAxisAlignment, MainAxisAlignment};

/// A widget that lays out children in a flex layout.
///
/// This is the base widget for both Row and Column.
/// Most users should use Row or Column directly.
///
/// ## Layout Behavior
///
/// - Children are laid out sequentially along the main axis
/// - Main axis: horizontal for Row, vertical for Column
/// - Cross axis: vertical for Row, horizontal for Column
///
/// ## Alignment Options
///
/// - `main_axis_alignment`: How children are positioned along the main axis
/// - `cross_axis_alignment`: How children are positioned along the cross axis
/// - `spacing`: Gap between adjacent children
#[derive(Debug)]
pub struct Flex {
    /// Direction of layout.
    pub direction: FlexDirection,
    /// Main axis alignment.
    pub main_axis_alignment: MainAxisAlignment,
    /// Cross axis alignment.
    pub cross_axis_alignment: CrossAxisAlignment,
    /// Spacing between children.
    pub spacing: f32,
    /// The children.
    children: Children,
}

impl Clone for Flex {
    fn clone(&self) -> Self {
        Self {
            direction: self.direction,
            main_axis_alignment: self.main_axis_alignment,
            cross_axis_alignment: self.cross_axis_alignment,
            spacing: self.spacing,
            children: self.children.clone(),
        }
    }
}

impl Flex {
    /// Creates a new Flex with the given direction.
    pub fn new(direction: FlexDirection) -> Self {
        Self {
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Start,
            spacing: 0.0,
            children: Children::new(),
        }
    }

    /// Creates a horizontal Flex (Row).
    pub fn row() -> Self {
        Self::new(FlexDirection::Horizontal)
    }

    /// Creates a vertical Flex (Column).
    pub fn column() -> Self {
        Self::new(FlexDirection::Vertical)
    }

    /// Sets the main axis alignment.
    pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    /// Sets the cross axis alignment.
    pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    /// Sets the spacing between children.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Sets the children from an array of views.
    pub fn children<V: View, const N: usize>(mut self, views: [V; N]) -> Self {
        self.children = views.into_iter().collect();
        self
    }

    /// Adds a single child.
    pub fn child(mut self, view: impl View) -> Self {
        self.children.push(view);
        self
    }
}

impl Default for Flex {
    fn default() -> Self {
        Self::row()
    }
}

// Implement View trait via macro
impl_render_view!(Flex);

impl RenderView for Flex {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(&self) -> Self::RenderObject {
        match self.direction {
            FlexDirection::Horizontal => RenderFlex::row(),
            FlexDirection::Vertical => RenderFlex::column(),
        }
        .with_main_axis_alignment(self.main_axis_alignment)
        .with_cross_axis_alignment(self.cross_axis_alignment)
        .with_spacing(self.spacing)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        if render_object.direction() != self.direction {
            // Direction changed - need to recreate
            *render_object = self.create_render_object();
        }
        // Note: alignment and spacing updates would require mutable setters on RenderFlex
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for child in self.children.iter() {
            visitor(child);
        }
    }
}

/// A widget that lays out children horizontally.
///
/// This is a convenience wrapper around Flex with horizontal direction.
///
/// ## Examples
///
/// ```rust,ignore
/// Row::new()
///     .spacing(8.0)
///     .main_axis_alignment(MainAxisAlignment::SpaceBetween)
///     .children([left_widget, center_widget, right_widget])
/// ```
#[derive(Debug)]
pub struct Row {
    inner: Flex,
}

impl Clone for Row {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Row {
    /// Creates a new Row.
    pub fn new() -> Self {
        Self { inner: Flex::row() }
    }

    /// Sets the main axis alignment (horizontal).
    pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.inner = self.inner.main_axis_alignment(alignment);
        self
    }

    /// Sets the cross axis alignment (vertical).
    pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.inner = self.inner.cross_axis_alignment(alignment);
        self
    }

    /// Sets the spacing between children.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.inner = self.inner.spacing(spacing);
        self
    }

    /// Sets the children from an array of views.
    pub fn children<V: View, const N: usize>(mut self, views: [V; N]) -> Self {
        self.inner = self.inner.children(views);
        self
    }

    /// Adds a single child.
    pub fn child(mut self, view: impl View) -> Self {
        self.inner = self.inner.child(view);
        self
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View trait via macro
impl_render_view!(Row);

impl RenderView for Row {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(&self) -> Self::RenderObject {
        self.inner.create_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        self.inner.update_render_object(render_object)
    }

    fn has_children(&self) -> bool {
        self.inner.has_children()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        self.inner.visit_child_views(visitor)
    }
}

/// A widget that lays out children vertically.
///
/// This is a convenience wrapper around Flex with vertical direction.
///
/// ## Examples
///
/// ```rust,ignore
/// Column::new()
///     .spacing(16.0)
///     .cross_axis_alignment(CrossAxisAlignment::Center)
///     .children([header, content, footer])
/// ```
#[derive(Debug)]
pub struct Column {
    inner: Flex,
}

impl Clone for Column {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Column {
    /// Creates a new Column.
    pub fn new() -> Self {
        Self {
            inner: Flex::column(),
        }
    }

    /// Sets the main axis alignment (vertical).
    pub fn main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.inner = self.inner.main_axis_alignment(alignment);
        self
    }

    /// Sets the cross axis alignment (horizontal).
    pub fn cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.inner = self.inner.cross_axis_alignment(alignment);
        self
    }

    /// Sets the spacing between children.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.inner = self.inner.spacing(spacing);
        self
    }

    /// Sets the children from an array of views.
    pub fn children<V: View, const N: usize>(mut self, views: [V; N]) -> Self {
        self.inner = self.inner.children(views);
        self
    }

    /// Adds a single child.
    pub fn child(mut self, view: impl View) -> Self {
        self.inner = self.inner.child(view);
        self
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View trait via macro
impl_render_view!(Column);

impl RenderView for Column {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFlex;

    fn create_render_object(&self) -> Self::RenderObject {
        self.inner.create_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        self.inner.update_render_object(render_object)
    }

    fn has_children(&self) -> bool {
        self.inner.has_children()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        self.inner.visit_child_views(visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_row() {
        let flex = Flex::row();
        assert_eq!(flex.direction, FlexDirection::Horizontal);
    }

    #[test]
    fn test_flex_column() {
        let flex = Flex::column();
        assert_eq!(flex.direction, FlexDirection::Vertical);
    }

    #[test]
    fn test_flex_with_alignment() {
        let flex = Flex::row()
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .spacing(8.0);

        assert_eq!(flex.main_axis_alignment, MainAxisAlignment::Center);
        assert_eq!(flex.cross_axis_alignment, CrossAxisAlignment::Stretch);
        assert_eq!(flex.spacing, 8.0);
    }

    #[test]
    fn test_row_new() {
        let row = Row::new();
        assert_eq!(row.inner.direction, FlexDirection::Horizontal);
    }

    #[test]
    fn test_column_new() {
        let column = Column::new();
        assert_eq!(column.inner.direction, FlexDirection::Vertical);
    }

    #[test]
    fn test_row_with_spacing() {
        let row = Row::new().spacing(16.0);
        assert_eq!(row.inner.spacing, 16.0);
    }

    #[test]
    fn test_column_with_alignment() {
        let column = Column::new()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Center);

        assert_eq!(
            column.inner.main_axis_alignment,
            MainAxisAlignment::SpaceBetween
        );
        assert_eq!(
            column.inner.cross_axis_alignment,
            CrossAxisAlignment::Center
        );
    }

    #[test]
    fn test_render_view_create() {
        let row = Row::new().spacing(8.0);
        let render = row.create_render_object();
        assert!(render.inner().is_horizontal());
    }
}
