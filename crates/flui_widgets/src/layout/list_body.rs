//! ListBody widget - simple scrollable list layout
//!
//! A widget that arranges children in a simple list along a main axis.
//! Unlike Flex, ListBody doesn't support flex factors - all children
//! are sized to their intrinsic size along the main axis.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, RenderNode};
use flui_rendering::RenderListBody;
use flui_types::Axis;

/// A widget that arranges children in a simple list.
///
/// ListBody is simpler than Flex (Row/Column) - it doesn't support
/// flex factors and just sizes each child to its intrinsic size.
/// Useful for simple scrollable lists.
///
/// ## Key Properties
///
/// - **main_axis**: Direction of layout (horizontal or vertical)
/// - **spacing**: Space between children (default: 0.0)
/// - **children**: List of child widgets
///
/// ## Common Use Cases
///
/// ### Vertical list
/// ```rust,ignore
/// ListBody::new(Axis::Vertical, vec![
///     Text::new("Item 1"),
///     Text::new("Item 2"),
///     Text::new("Item 3"),
/// ])
/// ```
///
/// ### Horizontal list with spacing
/// ```rust,ignore
/// ListBody::builder()
///     .main_axis(Axis::Horizontal)
///     .spacing(8.0)
///     .children(items)
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple vertical list
/// ListBody::new(Axis::Vertical, children)
///
/// // Horizontal list with spacing
/// ListBody::builder()
///     .main_axis(Axis::Horizontal)
///     .spacing(16.0)
///     .children(vec![widget1, widget2, widget3])
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_list_body)]
pub struct ListBody {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Main axis direction (horizontal or vertical)
    /// Default: Axis::Vertical
    #[builder(default = Axis::Vertical)]
    pub main_axis: Axis,

    /// Spacing between children
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub spacing: f32,

    /// The list of child widgets
    #[builder(default)]
    pub children: Vec<Widget>,
}

impl ListBody {
    /// Creates a new ListBody.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::new(Axis::Vertical, children);
    /// ```
    pub fn new(main_axis: Axis, children: Vec<Widget>) -> Self {
        Self {
            key: None,
            main_axis,
            spacing: 0.0,
            children,
        }
    }

    /// Creates a vertical ListBody.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::vertical(children);
    /// ```
    pub fn vertical(children: Vec<Widget>) -> Self {
        Self::new(Axis::Vertical, children)
    }

    /// Creates a horizontal ListBody.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::horizontal(children);
    /// ```
    pub fn horizontal(children: Vec<Widget>) -> Self {
        Self::new(Axis::Horizontal, children)
    }

    /// Creates a vertical ListBody with spacing.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::vertical_with_spacing(8.0, children);
    /// ```
    pub fn vertical_with_spacing(spacing: f32, children: Vec<Widget>) -> Self {
        Self {
            key: None,
            main_axis: Axis::Vertical,
            spacing,
            children,
        }
    }

    /// Creates a horizontal ListBody with spacing.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::horizontal_with_spacing(8.0, children);
    /// ```
    pub fn horizontal_with_spacing(spacing: f32, children: Vec<Widget>) -> Self {
        Self {
            key: None,
            main_axis: Axis::Horizontal,
            spacing,
            children,
        }
    }
}

impl Default for ListBody {
    fn default() -> Self {
        Self {
            key: None,
            main_axis: Axis::Vertical,
            spacing: 0.0,
            children: Vec::new(),
        }
    }
}

// bon Builder Extensions
use list_body_builder::State;

impl<S: State> ListBodyBuilder<S> {
    /// Builds the ListBody widget.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_list_body())
    }
}

// Implement RenderWidget
impl RenderWidget for ListBody {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let mut render = RenderListBody::new(self.main_axis);
        render.set_spacing(self.spacing);
        RenderNode::multi(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Multi { render, .. } = render_object {
            if let Some(list_body) = render.downcast_mut::<RenderListBody>() {
                list_body.set_main_axis(self.main_axis);
                list_body.set_spacing(self.spacing);
            }
        }
    }

    fn children(&self) -> Option<&[Widget]> {
        Some(&self.children)
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(ListBody, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_body_new() {
        let children = vec![Widget::from(()), Widget::from(())];
        let list = ListBody::new(Axis::Vertical, children.clone());
        assert_eq!(list.main_axis, Axis::Vertical);
        assert_eq!(list.spacing, 0.0);
        assert_eq!(list.children.len(), 2);
    }

    #[test]
    fn test_list_body_vertical() {
        let children = vec![Widget::from(())];
        let list = ListBody::vertical(children);
        assert_eq!(list.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_list_body_horizontal() {
        let children = vec![Widget::from(())];
        let list = ListBody::horizontal(children);
        assert_eq!(list.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_list_body_with_spacing() {
        let children = vec![Widget::from(())];
        let list = ListBody::vertical_with_spacing(8.0, children);
        assert_eq!(list.spacing, 8.0);
    }

    #[test]
    fn test_list_body_builder() {
        let list = ListBody::builder()
            .main_axis(Axis::Horizontal)
            .spacing(16.0)
            .build();
    }

    #[test]
    fn test_list_body_default() {
        let list = ListBody::default();
        assert_eq!(list.main_axis, Axis::Vertical);
        assert_eq!(list.spacing, 0.0);
        assert!(list.children.is_empty());
    }
}
