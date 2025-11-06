//! Flex widget - generic flex layout container
//!
//! A widget that displays its children in a one-dimensional array.
//! Similar to Flutter's Flex widget.
//!
//! This is the base widget for Row (horizontal) and Column (vertical).
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! Flex::builder()
//!     .direction(Axis::Horizontal)
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .children(vec![child1, child2])
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, MultiRenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderFlex;
use flui_types::layout::{Axis, CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

/// A widget that displays its children in a one-dimensional array.
///
/// Flex is the generic flex container that can be oriented horizontally or vertically.
/// For convenience, use Row (horizontal) or Column (vertical) instead of Flex directly.
///
/// ## Layout Behavior
///
/// - **Main axis**: The primary direction (horizontal or vertical)
/// - **Cross axis**: The perpendicular direction
/// - **Main axis size**: Can be `Max` (fill available space) or `Min` (shrink to children)
///
/// ## Examples
///
/// ```rust,ignore
/// // Horizontal flex (same as Row)
/// Flex::builder()
///     .direction(Axis::Horizontal)
///     .main_axis_alignment(MainAxisAlignment::SpaceBetween)
///     .children(vec![
///         Box::new(Text::new("Left")),
///         Box::new(Text::new("Right")),
///     ])
///     .build()
///
/// // Vertical flex (same as Column)
/// Flex::builder()
///     .direction(Axis::Vertical)
///     .main_axis_alignment(MainAxisAlignment::Center)
///     .children(vec![
///         Box::new(Text::new("Top")),
///         Box::new(Text::new("Bottom")),
///     ])
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Axis, into),
    on(MainAxisAlignment, into),
    on(CrossAxisAlignment, into),
    on(MainAxisSize, into),
    finish_fn = build_flex
)]
pub struct Flex {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The direction to use as the main axis.
    ///
    /// - Axis::Horizontal: Children laid out left-to-right (Row)
    /// - Axis::Vertical: Children laid out top-to-bottom (Column)
    #[builder(default = Axis::Horizontal)]
    pub direction: Axis,

    /// How children should be placed along the main axis.
    ///
    /// Defaults to MainAxisAlignment::Start if not specified.
    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    /// How children should be aligned along the cross axis.
    ///
    /// Defaults to CrossAxisAlignment::Center if not specified.
    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,

    /// How much space should be occupied in the main axis.
    ///
    /// - MainAxisSize::Max: Fills all available space (default)
    /// - MainAxisSize::Min: Shrinks to fit children
    #[builder(default = MainAxisSize::Max)]
    pub main_axis_size: MainAxisSize,

    /// The children widgets
    #[builder(default = vec![])]
    pub children: Vec<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Flex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flex")
            .field("key", &self.key)
            .field("direction", &self.direction)
            .field("main_axis_alignment", &self.main_axis_alignment)
            .field("cross_axis_alignment", &self.cross_axis_alignment)
            .field("main_axis_size", &self.main_axis_size)
            .field("children", &format!("[{} children]", self.children.len()))
            .finish()
    }
}

impl Clone for Flex {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            direction: self.direction,
            main_axis_alignment: self.main_axis_alignment,
            cross_axis_alignment: self.cross_axis_alignment,
            main_axis_size: self.main_axis_size,
            children: self.children.clone(),
        }
    }
}

impl Flex {
    /// Creates a new Flex widget with the specified direction.
    ///
    /// # Parameters
    ///
    /// - `direction`: The main axis direction (Horizontal or Vertical)
    pub fn new(direction: Axis) -> Self {
        Self {
            key: None,
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
            children: vec![],
        }
    }

    /// Creates a horizontal Flex (same as Row).
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Creates a vertical Flex (same as Column).
    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    /// Sets the children widgets.
    pub fn set_children(&mut self, children: Vec<Box<dyn AnyView>>) {
        self.children = children;
    }

    /// Adds a child widget.
    pub fn add_child(&mut self, child: Box<dyn AnyView>) {
        self.children.push(child);
    }
}

impl Default for Flex {
    fn default() -> Self {
        Self::new(Axis::Horizontal)
    }
}

// Implement View trait
impl View for Flex {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let render_flex = RenderFlex::new(self.direction)
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size);

        MultiRenderBuilder::new(render_flex).with_children(self.children)
    }
}

/// Macro for creating Flex with declarative syntax.
#[macro_export]
macro_rules! flex {
    (direction: $direction:expr) => {
        $crate::Flex::new($direction)
    };
    (horizontal) => {
        $crate::Flex::horizontal()
    };
    (vertical) => {
        $crate::Flex::vertical()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_new() {
        let widget = Flex::new(Axis::Horizontal);
        assert!(widget.key.is_none());
        assert_eq!(widget.direction, Axis::Horizontal);
        assert_eq!(widget.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(widget.cross_axis_alignment, CrossAxisAlignment::Center);
        assert_eq!(widget.main_axis_size, MainAxisSize::Max);
        assert!(widget.children.is_empty());
    }

    #[test]
    fn test_flex_horizontal() {
        let widget = Flex::horizontal();
        assert_eq!(widget.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_vertical() {
        let widget = Flex::vertical();
        assert_eq!(widget.direction, Axis::Vertical);
    }

    #[test]
    fn test_flex_default() {
        let widget = Flex::default();
        assert_eq!(widget.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_builder() {
        let widget = Flex::builder()
            .direction(Axis::Vertical)
            .main_axis_alignment(MainAxisAlignment::Center)
            .build_flex();
        assert_eq!(widget.direction, Axis::Vertical);
        assert_eq!(widget.main_axis_alignment, MainAxisAlignment::Center);
    }

    #[test]
    fn test_flex_add_child() {
        let mut widget = Flex::new(Axis::Horizontal);
        widget.add_child(Box::new(crate::SizedBox::new()));
        assert_eq!(widget.children.len(), 1);
    }

    #[test]
    fn test_flex_macro_horizontal() {
        let widget = flex!(horizontal);
        assert_eq!(widget.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_macro_vertical() {
        let widget = flex!(vertical);
        assert_eq!(widget.direction, Axis::Vertical);
    }

    #[test]
    fn test_flex_macro_direction() {
        let widget = flex!(direction: Axis::Horizontal);
        assert_eq!(widget.direction, Axis::Horizontal);
    }
}
