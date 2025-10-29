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
use flui_core::{BoxedWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::{RenderClipRect, RectShape, SingleArity};
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
    pub child: Option<BoxedWidget>,
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
    pub fn set_child<W: Widget + 'static>(&mut self, child: W) {
        self.child = Some(BoxedWidget::new(child));
    }
}

impl Default for ClipRect {
    fn default() -> Self {
        Self::new(Clip::AntiAlias)
    }
}

// Implement Widget trait with associated type


// bon Builder Extensions
use clip_rect_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> ClipRectBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child<W: Widget + 'static>(self, child: W) -> ClipRectBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Build wrapper
impl<S: State> ClipRectBuilder<S> {
    /// Builds the ClipRect widget.
    pub fn build(self) -> ClipRect {
        self.build_clip_rect()
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for ClipRect {
    type RenderObject = RenderClipRect;
    type Arity = SingleArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderClipRect::new(RectShape, self.clip_behavior)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_clip_behavior(self.clip_behavior);
    }
}

impl SingleChildRenderObjectWidget for ClipRect {
    fn child(&self) -> &BoxedWidget {
        self.child
            .as_ref()
            .unwrap_or_else(|| panic!("ClipRect requires a child"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    #[derive(Debug, Clone)]
    struct MockWidget;

    

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

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

    #[test]
    fn test_clip_rect_widget_trait() {
        let widget = ClipRect::builder()
            .clip_behavior(Clip::HardEdge)
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_clip_rect_render_object_creation() {
        let widget = ClipRect::new(Clip::AntiAlias);
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderClipRect>().is_some());
    }

    #[test]
    fn test_clip_rect_render_object_update() {
        let widget1 = ClipRect::new(Clip::HardEdge);
        let mut render_object = widget1.create_render_object();

        let widget2 = ClipRect::new(Clip::AntiAlias);
        widget2.update_render_object(&mut *render_object);

        let clip_render = render_object.downcast_ref::<RenderClipRect>().unwrap();
        assert_eq!(clip_render.clip_behavior(), Clip::AntiAlias);
    }
}
