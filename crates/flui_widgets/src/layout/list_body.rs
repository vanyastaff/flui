//! ListBody widget - simple scrollable list layout
//!
//! A widget that arranges children in a simple list along a main axis.
//! Unlike Flex, ListBody doesn't support flex factors - all children
//! are sized to their intrinsic size along the main axis.

use bon::Builder;
use flui_core::element::Element;
use flui_core::render::RenderBoxExt;
use flui_core::view::{IntoElement, View};

use flui_core::BuildContext;
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
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
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
    pub children: Vec<Element>,
}

impl std::fmt::Debug for ListBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListBody")
            .field("key", &self.key)
            .field("main_axis", &self.main_axis)
            .field("spacing", &self.spacing)
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

impl ListBody {
    /// Creates a new ListBody.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::new(Axis::Vertical, children);
    /// ```
    pub fn new(main_axis: Axis, children: Vec<Element>) -> Self {
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
    pub fn vertical(children: Vec<Element>) -> Self {
        Self::new(Axis::Vertical, children)
    }

    /// Creates a horizontal ListBody.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::horizontal(children);
    /// ```
    pub fn horizontal(children: Vec<Element>) -> Self {
        Self::new(Axis::Horizontal, children)
    }

    /// Creates a vertical ListBody with spacing.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let list = ListBody::vertical_with_spacing(8.0, children);
    /// ```
    pub fn vertical_with_spacing(spacing: f32, children: Vec<Element>) -> Self {
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
    pub fn horizontal_with_spacing(spacing: f32, children: Vec<Element>) -> Self {
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
    pub fn build(self) -> ListBody {
        self.build_internal()
    }
}

// Implement View trait
impl View for ListBody {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut render = RenderListBody::new(self.main_axis);
        render.set_spacing(self.spacing);

        render.children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_body_new() {
        let children = vec![
            crate::SizedBox::new().into_element(),
            crate::SizedBox::new().into_element(),
        ];
        let list = ListBody::new(Axis::Vertical, children);
        assert_eq!(list.main_axis, Axis::Vertical);
        assert_eq!(list.spacing, 0.0);
        assert_eq!(list.children.len(), 2);
    }

    #[test]
    fn test_list_body_vertical() {
        let children = vec![crate::SizedBox::new().into_element()];
        let list = ListBody::vertical(children);
        assert_eq!(list.main_axis, Axis::Vertical);
    }

    #[test]
    fn test_list_body_horizontal() {
        let children = vec![crate::SizedBox::new().into_element()];
        let list = ListBody::horizontal(children);
        assert_eq!(list.main_axis, Axis::Horizontal);
    }

    #[test]
    fn test_list_body_with_spacing() {
        let children = vec![crate::SizedBox::new().into_element()];
        let list = ListBody::vertical_with_spacing(8.0, children);
        assert_eq!(list.spacing, 8.0);
    }

    #[test]
    fn test_list_body_builder() {
        let list = ListBody::builder()
            .main_axis(Axis::Horizontal)
            .spacing(16.0)
            .build();
        assert_eq!(list.main_axis, Axis::Horizontal);
        assert_eq!(list.spacing, 16.0);
    }

    #[test]
    fn test_list_body_default() {
        let list = ListBody::default();
        assert_eq!(list.main_axis, Axis::Vertical);
        assert_eq!(list.spacing, 0.0);
        assert!(list.children.is_empty());
    }
}
