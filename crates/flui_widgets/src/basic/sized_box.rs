//! SizedBox widget - forces specific size constraints
//!
//! A widget that forces its child to have a specific size.
//! Similar to Flutter's SizedBox widget.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Fixed size box
//! SizedBox::fixed(100.0, 50.0).child(content)
//!
//! // Expand to fill
//! SizedBox::expand().child(content)
//!
//! // Shrink to nothing
//! SizedBox::shrink()
//! ```

use flui_rendering::objects::RenderSizedBox;
use flui_rendering::wrapper::BoxWrapper;
use flui_view::{impl_render_view, Child, RenderView, View};

/// A widget that forces a specific size.
///
/// If width or height is None, that dimension is flexible and
/// will use the incoming constraints.
///
/// ## Layout Behavior
///
/// - Fixed dimensions override incoming constraints (clamped to valid range)
/// - None dimensions use the max constraint value
/// - Useful for creating spacing or forcing child sizes
///
/// ## Examples
///
/// ```rust,ignore
/// // Fixed 100x100 box
/// SizedBox::fixed(100.0, 100.0).child(content)
///
/// // Fixed width, flexible height
/// SizedBox::from_width(200.0).child(content)
///
/// // Expand to fill available space
/// SizedBox::expand().child(content)
///
/// // Empty spacer
/// SizedBox::fixed(20.0, 20.0)
/// ```
#[derive(Debug)]
pub struct SizedBox {
    /// Fixed width, or None for flexible.
    pub width: Option<f32>,
    /// Fixed height, or None for flexible.
    pub height: Option<f32>,
    /// The child widget.
    child: Child,
}

impl Clone for SizedBox {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            child: self.child.clone(),
        }
    }
}

impl SizedBox {
    /// Creates a SizedBox with optional fixed dimensions.
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            width,
            height,
            child: Child::empty(),
        }
    }

    /// Creates a SizedBox with fixed dimensions.
    pub fn fixed(width: f32, height: f32) -> Self {
        Self::new(Some(width), Some(height))
    }

    /// Creates a SizedBox that expands to fill available space.
    pub fn expand() -> Self {
        Self::new(None, None)
    }

    /// Creates a SizedBox that shrinks to zero size.
    pub fn shrink() -> Self {
        Self::fixed(0.0, 0.0)
    }

    /// Creates a square SizedBox.
    pub fn square(dimension: f32) -> Self {
        Self::fixed(dimension, dimension)
    }

    /// Creates a SizedBox with only fixed width.
    pub fn from_width(width: f32) -> Self {
        Self::new(Some(width), None)
    }

    /// Creates a SizedBox with only fixed height.
    pub fn from_height(height: f32) -> Self {
        Self::new(None, Some(height))
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }
}

impl Default for SizedBox {
    fn default() -> Self {
        Self::expand()
    }
}

// Implement View trait via macro
impl_render_view!(SizedBox);

impl RenderView for SizedBox {
    type RenderObject = BoxWrapper<RenderSizedBox>;

    fn create_render_object(&self) -> Self::RenderObject {
        BoxWrapper::new(RenderSizedBox::new(self.width, self.height))
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        let inner = render_object.inner();
        if inner.width() != self.width || inner.height() != self.height {
            *render_object = BoxWrapper::new(RenderSizedBox::new(self.width, self.height));
        }
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sized_box_fixed() {
        let sized = SizedBox::fixed(100.0, 50.0);
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(50.0));
    }

    #[test]
    fn test_sized_box_expand() {
        let sized = SizedBox::expand();
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_sized_box_shrink() {
        let sized = SizedBox::shrink();
        assert_eq!(sized.width, Some(0.0));
        assert_eq!(sized.height, Some(0.0));
    }

    #[test]
    fn test_sized_box_square() {
        let sized = SizedBox::square(50.0);
        assert_eq!(sized.width, Some(50.0));
        assert_eq!(sized.height, Some(50.0));
    }

    #[test]
    fn test_sized_box_from_width() {
        let sized = SizedBox::from_width(100.0);
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_sized_box_from_height() {
        let sized = SizedBox::from_height(100.0);
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, Some(100.0));
    }

    #[test]
    fn test_render_view_create() {
        let sized = SizedBox::fixed(100.0, 50.0);
        let render = sized.create_render_object();
        assert_eq!(render.inner().width(), Some(100.0));
        assert_eq!(render.inner().height(), Some(50.0));
    }
}
