//! ClipOval widget - clips child to an oval shape
//!
//! A widget that clips its child to an oval shape.
//! Similar to Flutter's ClipOval widget.

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::RenderClipOval;
use flui_types::painting::Clip;

/// A widget that clips its child to an oval shape.
///
/// ClipOval clips its child using an oval (ellipse) that inscribes the widget's bounds.
/// If the widget is square, the result is a perfect circle. If rectangular, it's an ellipse.
///
/// ## Clip Behavior
///
/// - **Clip::None**: No clipping (most efficient, but content may overflow)
/// - **Clip::HardEdge**: Clips without anti-aliasing (faster, may look jagged)
/// - **Clip::AntiAlias**: Clips with anti-aliasing (default, smooth edges)
/// - **Clip::AntiAliasWithSaveLayer**: Anti-aliased with save layer (slowest, best quality)
///
/// ## Common Use Cases
///
/// ### Circular avatar
/// ```rust,ignore
/// SizedBox::builder()
///     .width(50.0)
///     .height(50.0)
///     .child(ClipOval::new(Image::new("avatar.jpg")))
///     .build()
/// ```
///
/// ### Circular button
/// ```rust,ignore
/// ClipOval::builder()
///     .child(Container::builder()
///         .width(60.0)
///         .height(60.0)
///         .color(Color::BLUE)
///         .child(Icon::new("add"))
///         .build())
///     .build()
/// ```
///
/// ### Elliptical image crop
/// ```rust,ignore
/// SizedBox::builder()
///     .width(100.0)
///     .height(60.0)  // Non-square = ellipse
///     .child(ClipOval::new(Image::new("photo.jpg")))
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple circular clip (default anti-aliasing)
/// ClipOval::new(child_widget)
///
/// // Hard edge clipping (faster)
/// ClipOval::builder()
///     .clip_behavior(Clip::HardEdge)
///     .child(widget)
///     .build()
///
/// // Explicit anti-aliasing
/// ClipOval::builder()
///     .clip_behavior(Clip::AntiAlias)
///     .child(image)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), on(Clip, into), finish_fn = build_clip_oval)]
pub struct ClipOval {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How to clip the child.
    /// Default: Clip::AntiAlias (smooth edges)
    #[builder(default = Clip::AntiAlias)]
    pub clip_behavior: Clip,

    /// The child widget to clip.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl ClipOval {
    /// Creates a new ClipOval with default anti-aliased clipping.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ClipOval::new(Image::new("avatar.jpg"));
    /// ```
    pub fn new(child: Widget) -> Self {
        Self {
            key: None,
            clip_behavior: Clip::AntiAlias,
            child: Some(child),
        }
    }

    /// Creates a ClipOval with specified clip behavior.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ClipOval::with_clip(Clip::HardEdge, child);
    /// ```
    pub fn with_clip(clip_behavior: Clip, child: Widget) -> Self {
        Self {
            key: None,
            clip_behavior,
            child: Some(child),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }
}

impl Default for ClipOval {
    fn default() -> Self {
        Self {
            key: None,
            clip_behavior: Clip::AntiAlias,
            child: None,
        }
    }
}

// bon Builder Extensions
use clip_oval_builder::{IsUnset, SetChild, State};

impl<S: State> ClipOvalBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: Widget) -> ClipOvalBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

impl<S: State> ClipOvalBuilder<S> {
    /// Builds the ClipOval widget.
    pub fn build(self) -> ClipOval {
        self.build_clip_oval()
    }
}

// Implement RenderWidget
impl RenderWidget for ClipOval {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderClipOval::with_clip(self.clip_behavior)))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(clip_oval) = render.downcast_mut::<RenderClipOval>() {
                clip_oval.set_clip_behavior(self.clip_behavior);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(ClipOval, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_oval_new() {
        let widget = ClipOval::new(Widget::from(()));
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_clip_oval_with_clip() {
        let widget = ClipOval::with_clip(Clip::HardEdge, Widget::from(()));
        assert_eq!(widget.clip_behavior, Clip::HardEdge);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_clip_oval_builder() {
        let widget = ClipOval::builder()
            .clip_behavior(Clip::None)
            .build();
        assert_eq!(widget.clip_behavior, Clip::None);
    }

    #[test]
    fn test_clip_oval_default() {
        let widget = ClipOval::default();
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_clip_oval_set_child() {
        let mut widget = ClipOval::default();
        assert!(widget.child.is_none());

        widget.set_child(Widget::from(()));
        assert!(widget.child.is_some());
    }
}
