//! [`Padding`] — insets its child by a given amount.

use flui_foundation::geometry::EdgeInsets;
use flui_objects::RenderPadding;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// A widget that insets its child by the given [`EdgeInsets`].
///
/// Backed by `RenderPadding`. The
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
    insets: EdgeInsets,
    child: Child,
    site: CreationSite,
}

impl Padding {
    /// Create padding from explicit [`EdgeInsets`], with no child yet.
    #[track_caller]
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            insets: padding,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Uniform padding on all four sides.
    #[track_caller]
    pub fn all(value: f64) -> Self {
        Self::new(EdgeInsets::all(value))
    }

    /// Symmetric padding: `horizontal` on left/right, `vertical` on top/bottom.
    #[track_caller]
    pub fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self::new(EdgeInsets::symmetric(vertical, horizontal))
    }

    /// Padding on individually-named sides (unspecified sides are zero).
    #[track_caller]
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
        RenderPadding::new(self.insets)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_padding(self.insets);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Padding, site = site);
