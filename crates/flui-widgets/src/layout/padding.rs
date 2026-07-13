//! [`Padding`] — insets its child by a given amount.

use flui_geometry::{EdgeInsets, px};
use flui_objects::RenderPadding;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// A widget that insets its child by the given [`EdgeInsets`].
///
/// Flutter parity: `widgets/basic.dart` `Padding` over `RenderPadding`. The
/// child is laid out inside the constraints deflated by the padding, then the
/// padding is added back to the child's size to produce this widget's size.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = Padding::all(8.0).child(Text::new("hello"));
/// ```
#[derive(Clone, Debug)]
pub struct Padding {
    padding: EdgeInsets,
    child: Child,
}

impl Padding {
    /// Create padding from explicit [`EdgeInsets`], with no child yet.
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            child: Child::empty(),
        }
    }

    /// Uniform padding on all four sides.
    pub fn all(value: f32) -> Self {
        Self::new(EdgeInsets::all(px(value)))
    }

    /// Symmetric padding: `horizontal` on left/right, `vertical` on top/bottom.
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(px(vertical), px(horizontal)))
    }

    /// Padding on individually-named sides (unspecified sides are zero).
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::new(EdgeInsets::new(px(top), px(right), px(bottom), px(left)))
    }

    /// Set the child laid out inside the padding.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Padding {
    type Protocol = BoxProtocol;
    type RenderObject = RenderPadding;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderPadding::new(self.padding)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_padding(self.padding);
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(Padding);
