//! ClipRect widget - clips child to a rectangle
//!
//! A widget that clips its child using a rectangle.
//! Similar to Flutter's ClipRect widget.
//!
//! # Usage Patterns
//!
//! ## Builder Pattern
//! ```rust,ignore
//! ClipRect::builder()
//!     .clip_behavior(Clip::AntiAlias)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::{RenderObject, RenderObjectWidget, Widget};
use flui_rendering::RenderClipRect;
use flui_types::painting::Clip;

/// A widget that clips its child using a rectangle.
///
/// By default, ClipRect prevents its child from painting outside its bounds,
/// but the size and location of the clip rect match those of the child,
/// so it doesn't affect layout.
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - No effect on layout, only affects painting
///
/// ## Clipping Behavior
///
/// - **Clip::None**: No clipping (child paints normally)
/// - **Clip::HardEdge**: Fast clipping without anti-aliasing
/// - **Clip::AntiAlias**: Smooth clipping with anti-aliasing (slower)
/// - **Clip::AntiAliasWithSaveLayer**: Highest quality but slowest
///
/// ## Examples
///
/// ```rust,ignore
/// // Clip with anti-aliasing
/// ClipRect::builder()
///     .clip_behavior(Clip::AntiAlias)
///     .child(overflowing_content)
///     .build()
///
/// // Fast clipping without anti-aliasing
/// ClipRect::builder()
///     .clip_behavior(Clip::HardEdge)
///     .child(content)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_clip_rect)]
pub struct ClipRect {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How to clip the child
    #[builder(default = Clip::AntiAlias)]
    pub clip_behavior: Clip,

    /// The child widget to clip
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn Widget>>,
}

impl ClipRect {
    /// Creates a new ClipRect widget.
    ///
    /// # Parameters
    ///
    /// - `clip_behavior`: How to perform clipping (default: AntiAlias)
    pub fn new(clip_behavior: Clip) -> Self {
        Self {
            key: None,
            clip_behavior,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl Widget + 'static) {
        self.child = Some(Box::new(child));
    }
}

impl Default for ClipRect {
    fn default() -> Self {
        Self::new(Clip::AntiAlias)
    }
}

impl Widget for ClipRect {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        Box::new(flui_core::RenderObjectElement::new(self.clone()))
    }
}

// bon Builder Extensions
use clip_rect_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> ClipRectBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl Widget + 'static) -> ClipRectBuilder<SetChild<S>> {
        self.child_internal(Box::new(child) as Box<dyn Widget>)
    }
}

// Build wrapper
impl<S: State> ClipRectBuilder<S> {
    /// Builds the ClipRect widget.
    pub fn build(self) -> ClipRect {
        self.build_clip_rect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct MockWidget;
    impl Widget for MockWidget {
        fn create_element(&self) -> Box<dyn flui_core::Element> {
            todo!()
        }
    }

    #[test]
    fn test_clip_rect_new() {
        let widget = ClipRect::new(Clip::HardEdge);
        assert!(widget.key.is_none());
        assert_eq!(widget.clip_behavior, Clip::HardEdge);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_clip_rect_default() {
        let widget = ClipRect::default();
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rect_builder() {
        let widget = ClipRect::builder().build();
        assert_eq!(widget.clip_behavior, Clip::AntiAlias); // Default
    }

    #[test]
    fn test_clip_rect_builder_with_child() {
        let widget = ClipRect::builder().child(MockWidget).build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_clip_rect_builder_with_clip_behavior() {
        let widget = ClipRect::builder()
            .clip_behavior(Clip::None)
            .build();
        assert_eq!(widget.clip_behavior, Clip::None);
    }

    #[test]
    fn test_clip_rect_set_child() {
        let mut widget = ClipRect::new(Clip::HardEdge);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_clip_rect_all_clip_behaviors() {
        // Test all clip behavior variants
        let widget_none = ClipRect::new(Clip::None);
        assert_eq!(widget_none.clip_behavior, Clip::None);

        let widget_hard = ClipRect::new(Clip::HardEdge);
        assert_eq!(widget_hard.clip_behavior, Clip::HardEdge);

        let widget_aa = ClipRect::new(Clip::AntiAlias);
        assert_eq!(widget_aa.clip_behavior, Clip::AntiAlias);

        let widget_save = ClipRect::new(Clip::AntiAliasWithSaveLayer);
        assert_eq!(widget_save.clip_behavior, Clip::AntiAliasWithSaveLayer);
    }
}

impl RenderObjectWidget for ClipRect {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        Box::new(RenderClipRect::new(self.clip_behavior))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        if let Some(clip) = render_object.downcast_mut::<RenderClipRect>() {
            clip.set_clip_behavior(self.clip_behavior);
        }
    }
}
