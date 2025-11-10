//! Viewport - A widget that displays a slice of large content
//!
//! Based on Flutter's Viewport. Shows a portion of content through a
//! fixed-size window, applying an offset to show different parts.

use flui_core::view::{AnyView, BuildContext, IntoElement, View};
use flui_rendering::objects::RenderViewport;
use flui_types::layout::Axis;
use flui_types::Offset;

/// A widget that displays a slice of content through a fixed viewport
///
/// The viewport shows a portion of its child based on the offset.
/// The child can be larger than the viewport and will be clipped.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::Viewport;
/// use flui_types::layout::Axis;
/// use flui_types::Offset;
///
/// Viewport::builder()
///     .axis(Axis::Vertical)
///     .offset(Offset::new(0.0, 100.0))
///     .child(large_content_widget)
///     .build()
/// ```
#[derive(Clone)]
pub struct Viewport {
    /// The child widget to display
    pub child: Box<dyn AnyView>,

    /// The axis along which to scroll
    pub axis: Axis,

    /// The current scroll offset
    pub offset: Offset,

    /// Whether to clip content outside viewport
    pub clip: bool,
}

impl Viewport {
    /// Create a new Viewport
    pub fn new(child: impl View + 'static) -> Self {
        Self {
            child: Box::new(child),
            axis: Axis::Vertical,
            offset: Offset::ZERO,
            clip: true,
        }
    }

    /// Create a vertical viewport
    pub fn vertical(child: impl View + 'static) -> Self {
        Self::new(child)
    }

    /// Create a horizontal viewport
    pub fn horizontal(child: impl View + 'static) -> Self {
        Self {
            child: Box::new(child),
            axis: Axis::Horizontal,
            offset: Offset::ZERO,
            clip: true,
        }
    }

    /// Set the viewport axis
    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Set the viewport offset
    pub fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Set whether to clip content
    pub fn with_clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    /// Builder for Viewport
    pub fn builder() -> ViewportBuilder {
        ViewportBuilder::new()
    }
}

impl std::fmt::Debug for Viewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewport")
            .field("axis", &self.axis)
            .field("offset", &self.offset)
            .field("clip", &self.clip)
            .field("child", &"<Widget>")
            .finish()
    }
}

impl View for Viewport {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut render = RenderViewport::new(self.axis, self.offset);
        render.set_clip(self.clip);

        (render, Some(self.child))
    }
}

/// Builder for Viewport
pub struct ViewportBuilder {
    child: Option<Box<dyn AnyView>>,
    axis: Axis,
    offset: Offset,
    clip: bool,
}

impl ViewportBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            child: None,
            axis: Axis::Vertical,
            offset: Offset::ZERO,
            clip: true,
        }
    }

    /// Set the child widget
    pub fn child(mut self, child: impl View + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Set the viewport axis
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Set the viewport offset
    pub fn offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Set whether to clip content
    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    /// Build the Viewport
    pub fn build(self) -> Viewport {
        Viewport {
            child: self.child.expect("Viewport requires a child"),
            axis: self.axis,
            offset: self.offset,
            clip: self.clip,
        }
    }
}

impl Default for ViewportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_new() {
        // We can't easily test View construction without a full element tree,
        // so just test the struct construction
        let viewport = Viewport::vertical(());
        assert_eq!(viewport.axis, Axis::Vertical);
        assert_eq!(viewport.offset, Offset::ZERO);
        assert!(viewport.clip);
    }

    #[test]
    fn test_viewport_builder() {
        let viewport = Viewport::builder()
            .axis(Axis::Horizontal)
            .offset(Offset::new(50.0, 0.0))
            .clip(false)
            .child(())
            .build();

        assert_eq!(viewport.axis, Axis::Horizontal);
        assert_eq!(viewport.offset, Offset::new(50.0, 0.0));
        assert!(!viewport.clip);
    }

    #[test]
    fn test_viewport_with_methods() {
        let viewport = Viewport::new(())
            .with_axis(Axis::Horizontal)
            .with_offset(Offset::new(10.0, 20.0))
            .with_clip(false);

        assert_eq!(viewport.axis, Axis::Horizontal);
        assert_eq!(viewport.offset, Offset::new(10.0, 20.0));
        assert!(!viewport.clip);
    }
}
