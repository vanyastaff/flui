//! Padding widget - adds empty space around a child
//!
//! A widget that insets its child by the given padding.
//! Similar to Flutter's Padding widget.
//!
//! # Usage Patterns
//!
//! ## 1. Convenience Methods (Recommended)
//! ```rust,ignore
//! // Uniform padding on all sides
//! Padding::all(16.0).child(text_widget)
//!
//! // Symmetric padding (horizontal, vertical)
//! Padding::symmetric(20.0, 10.0).child(content)
//!
//! // Only specific sides
//! Padding::only(10.0, 0.0, 0.0, 0.0).child(content)
//! ```

use flui_rendering::objects::RenderPadding;
use flui_rendering::wrapper::BoxWrapper;
use flui_types::EdgeInsets;
use flui_view::{impl_render_view, Child, RenderView, View};

/// A widget that insets its child by the given padding.
///
/// ## Layout Behavior
///
/// - The padding is applied inside any decoration constraints
/// - Negative padding is not supported and will be clamped to zero
/// - The child size is reduced by the padding amount
///
/// ## Examples
///
/// ```rust,ignore
/// // Uniform padding
/// Padding::all(16.0).child(Text::new("Hello"))
///
/// // Asymmetric padding
/// Padding::only(10.0, 5.0, 10.0, 5.0).child(content)
/// ```
#[derive(Debug)]
pub struct Padding {
    /// The amount of space by which to inset the child.
    pub padding: EdgeInsets,

    /// The child widget.
    child: Child,
}

impl Clone for Padding {
    fn clone(&self) -> Self {
        Self {
            padding: self.padding,
            child: self.child.clone(),
        }
    }
}

impl Padding {
    /// Creates a new Padding with zero padding.
    pub fn new() -> Self {
        Self {
            padding: EdgeInsets::ZERO,
            child: Child::empty(),
        }
    }

    /// Creates a Padding with the given EdgeInsets.
    pub fn from_insets(padding: EdgeInsets) -> Self {
        Self {
            padding,
            child: Child::empty(),
        }
    }

    /// Creates a Padding with uniform padding on all sides.
    pub fn all(value: f32) -> Self {
        Self {
            padding: EdgeInsets::all(value),
            child: Child::empty(),
        }
    }

    /// Creates a Padding with symmetric horizontal and vertical padding.
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            padding: EdgeInsets::symmetric(horizontal, vertical),
            child: Child::empty(),
        }
    }

    /// Creates a Padding with custom padding on specific sides.
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            padding: EdgeInsets::new(left, top, right, bottom),
            child: Child::empty(),
        }
    }

    /// Creates a Padding with only left padding.
    pub fn left(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(value, 0.0, 0.0, 0.0),
            child: Child::empty(),
        }
    }

    /// Creates a Padding with only top padding.
    pub fn top(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(0.0, value, 0.0, 0.0),
            child: Child::empty(),
        }
    }

    /// Creates a Padding with only right padding.
    pub fn right(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(0.0, 0.0, value, 0.0),
            child: Child::empty(),
        }
    }

    /// Creates a Padding with only bottom padding.
    pub fn bottom(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(0.0, 0.0, 0.0, value),
            child: Child::empty(),
        }
    }

    /// Creates horizontal padding (left and right).
    pub fn horizontal(value: f32) -> Self {
        Self {
            padding: EdgeInsets::symmetric(value, 0.0),
            child: Child::empty(),
        }
    }

    /// Creates vertical padding (top and bottom).
    pub fn vertical(value: f32) -> Self {
        Self {
            padding: EdgeInsets::symmetric(0.0, value),
            child: Child::empty(),
        }
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View trait via macro (creates create_element and as_any)
impl_render_view!(Padding);

impl RenderView for Padding {
    type RenderObject = BoxWrapper<RenderPadding>;

    fn create_render_object(&self) -> Self::RenderObject {
        BoxWrapper::new(RenderPadding::new(self.padding))
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        if render_object.inner().padding() != self.padding {
            render_object.inner_mut().set_padding(self.padding);
        }
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child_view) = self.child.as_ref() {
            visitor(child_view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_new() {
        let padding = Padding::new();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_all() {
        let padding = Padding::all(16.0);
        assert_eq!(padding.padding, EdgeInsets::all(16.0));
    }

    #[test]
    fn test_padding_symmetric() {
        let padding = Padding::symmetric(20.0, 10.0);
        assert_eq!(padding.padding, EdgeInsets::symmetric(20.0, 10.0));
    }

    #[test]
    fn test_padding_only() {
        let padding = Padding::only(1.0, 2.0, 3.0, 4.0);
        assert_eq!(padding.padding, EdgeInsets::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn test_padding_left() {
        let padding = Padding::left(10.0);
        assert_eq!(padding.padding.left, 10.0);
        assert_eq!(padding.padding.top, 0.0);
        assert_eq!(padding.padding.right, 0.0);
        assert_eq!(padding.padding.bottom, 0.0);
    }

    #[test]
    fn test_padding_default() {
        let padding = Padding::default();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_render_view_create() {
        let padding = Padding::all(10.0);
        let render = padding.create_render_object();
        // Access inner RenderPadding through BoxWrapper
        assert_eq!(render.inner().padding(), EdgeInsets::all(10.0));
    }
}
