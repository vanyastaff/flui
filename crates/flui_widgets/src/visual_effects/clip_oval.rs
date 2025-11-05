//! ClipOval widget - clips child to an oval shape
//!
//! A widget that clips its child to an oval shape.
//! Similar to Flutter's ClipOval widget.

use bon::Builder;
use flui_core::{BuildContext, Element, RenderElement};
use flui_core::render::RenderNode;
use flui_core::view::{View, ChangeFlags, AnyView};
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
#[derive(Builder)]
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
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for ClipOval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipOval")
            .field("key", &self.key)
            .field("clip_behavior", &self.clip_behavior)
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for ClipOval {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            clip_behavior: self.clip_behavior,
            child: self.child.clone(),
        }
    }
}

impl ClipOval {
    /// Creates a new ClipOval with default anti-aliased clipping.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ClipOval::new(Image::new("avatar.jpg"));
    /// ```
    pub fn new(child: impl View + 'static) -> Self {
        Self {
            key: None,
            clip_behavior: Clip::AntiAlias,
            child: Some(Box::new(child)),
        }
    }

    /// Creates a ClipOval with specified clip behavior.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ClipOval::with_clip(Clip::HardEdge, child);
    /// ```
    pub fn with_clip(clip_behavior: Clip, child: impl View + 'static) -> Self {
        Self {
            key: None,
            clip_behavior,
            child: Some(Box::new(child)),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
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

// Implement View for ClipOval - New architecture
impl View for ClipOval {
    type Element = Element;
    type State = Option<Box<dyn std::any::Any>>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child if present
        let (child_id, child_state) = if let Some(child) = self.child {
            let (elem, state) = child.build_any(ctx);
            let id = ctx.tree().write().insert(elem.into_element());
            (Some(id), Some(state))
        } else {
            (None, None)
        };

        // Create RenderNode (Single - child is Option<ElementId>)
        let render_node = RenderNode::Single {
            render: Box::new(RenderClipOval::with_clip(self.clip_behavior)),
            child: child_id,
        };

        // Create RenderElement using constructor
        let render_element = RenderElement::new(render_node);

        (Element::Render(render_element), child_state)
    }

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // TODO: Implement proper rebuild logic if needed
        // For now, return NONE as View architecture handles rebuilding
        ChangeFlags::NONE
    }
}

// bon Builder Extensions
use clip_oval_builder::{IsUnset, SetChild, State};

impl<S: State> ClipOvalBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> ClipOvalBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

impl<S: State> ClipOvalBuilder<S> {
    /// Builds the ClipOval widget.
    pub fn build(self) -> ClipOval {
        self.build_clip_oval()
    }
}

// ClipOval now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    // Mock view for testing
    #[derive()]
    struct MockView;

    impl View for MockView {
        type Element = Element;
        type State = ();

        fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
            use flui_rendering::RenderColoredBox;
            use flui_types::Color;
            let render_node = RenderNode::Leaf(Box::new(RenderColoredBox::new(Color::BLACK)));
            let render_element = RenderElement::new(render_node);
            (Element::Render(render_element), ())
        }

        fn rebuild(self, _prev: &Self, _state: &mut Self::State, _element: &mut Self::Element) -> ChangeFlags {
            ChangeFlags::NONE
        }
    }

    #[test]
    fn test_clip_oval_new() {
        let widget = ClipOval::new(MockView);
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_clip_oval_with_clip() {
        let widget = ClipOval::with_clip(Clip::HardEdge, MockView);
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

        widget.set_child(MockView);
        assert!(widget.child.is_some());
    }
}
