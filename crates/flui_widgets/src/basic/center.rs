//! Center widget - centers a child within available space
//!
//! A widget that centers its child within the available space.
//! Similar to Flutter's Center widget.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Center a child
//! Center::new().child(content)
//!
//! // Center with size factors
//! Center::new()
//!     .width_factor(0.5)
//!     .height_factor(0.5)
//!     .child(content)
//! ```

use flui_rendering::objects::RenderCenter;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{impl_render_view, Child, RenderView, View};

/// A widget that centers its child within the available space.
///
/// ## Layout Behavior
///
/// - The child is given loose constraints (can be any size up to parent's max)
/// - The child is then positioned at the center of the available space
/// - If no child is provided, Center expands to fill available space
///
/// ## Size Factors
///
/// Optional width_factor and height_factor can shrink the available space:
/// - factor of 1.0 uses child's natural size
/// - factor of 0.5 uses half the child's size
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple centering
/// Center::new().child(ColoredBox::red(50.0, 50.0))
///
/// // Centered with factors
/// Center::new()
///     .width_factor(2.0)  // 2x child width
///     .child(content)
/// ```
#[derive(Debug)]
pub struct Center {
    /// Width factor to scale child width.
    pub width_factor: Option<f32>,
    /// Height factor to scale child height.
    pub height_factor: Option<f32>,
    /// The child widget.
    child: Child,
}

impl Clone for Center {
    fn clone(&self) -> Self {
        Self {
            width_factor: self.width_factor,
            height_factor: self.height_factor,
            child: self.child.clone(),
        }
    }
}

impl Center {
    /// Creates a new Center widget.
    pub fn new() -> Self {
        Self {
            width_factor: None,
            height_factor: None,
            child: Child::empty(),
        }
    }

    /// Sets the width factor.
    pub fn width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Sets the height factor.
    pub fn height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View trait via macro
impl_render_view!(Center);

impl RenderView for Center {
    type Protocol = BoxProtocol;
    type RenderObject = RenderCenter;

    fn create_render_object(&self) -> Self::RenderObject {
        let mut render = RenderCenter::new();
        if let Some(wf) = self.width_factor {
            render = render.with_width_factor(wf);
        }
        if let Some(hf) = self.height_factor {
            render = render.with_height_factor(hf);
        }
        render
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        if render_object.width_factor() != self.width_factor
            || render_object.height_factor() != self.height_factor
        {
            let mut render = RenderCenter::new();
            if let Some(wf) = self.width_factor {
                render = render.with_width_factor(wf);
            }
            if let Some(hf) = self.height_factor {
                render = render.with_height_factor(hf);
            }
            *render_object = render;
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
    fn test_center_new() {
        let center = Center::new();
        assert_eq!(center.width_factor, None);
        assert_eq!(center.height_factor, None);
    }

    #[test]
    fn test_center_with_factors() {
        let center = Center::new().width_factor(0.5).height_factor(0.75);
        assert_eq!(center.width_factor, Some(0.5));
        assert_eq!(center.height_factor, Some(0.75));
    }

    #[test]
    fn test_center_default() {
        let center = Center::default();
        assert_eq!(center.width_factor, None);
        assert_eq!(center.height_factor, None);
    }

    #[test]
    fn test_render_view_create() {
        let center = Center::new().width_factor(0.5).height_factor(0.75);
        let render = center.create_render_object();
        assert_eq!(render.inner().width_factor(), Some(0.5));
        assert_eq!(render.inner().height_factor(), Some(0.75));
    }

    #[test]
    fn test_render_view_create_no_factors() {
        let center = Center::new();
        let render = center.create_render_object();
        assert_eq!(render.inner().width_factor(), None);
        assert_eq!(render.inner().height_factor(), None);
    }
}
