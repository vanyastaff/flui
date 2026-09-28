//! [`Padding`] — insets its child by a given amount.

use flui_geometry::EdgeInsets;
use flui_objects::RenderPadding;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

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
    pub fn all(value: f64) -> Self {
        Self::new(EdgeInsets::all(value))
    }

    /// Symmetric padding: `horizontal` on left/right, `vertical` on top/bottom.
    pub fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self::new(EdgeInsets::symmetric(vertical, horizontal))
    }

    /// Padding on individually-named sides (unspecified sides are zero).
    pub fn only(left: f64, top: f64, right: f64, bottom: f64) -> Self {
        Self::new(EdgeInsets::new(top, right, bottom, left))
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
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_padding(self.padding);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Padding);
