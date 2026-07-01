//! [`SliverFillViewport`] — a sliver that sizes each box child to a fraction of
//! the viewport's main-axis extent.

use std::fmt;

use flui_objects::RenderSliverFillViewport;
use flui_rendering::protocol::SliverProtocol;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// A sliver that sizes each of its eagerly-attached box children to
/// `viewport_fraction × viewport_main_axis_extent`.
///
/// With the default `viewport_fraction = 1.0` each child fills exactly one
/// full viewport page, making this the backing primitive for page-view style
/// layouts. Set a smaller fraction (e.g. `0.9`) to peek at adjacent children.
///
/// Flutter parity: `widgets/sliver.dart` `SliverFillViewport` over
/// `RenderSliverFillViewport`. Lives inside a [`Viewport`](crate::Viewport).
///
/// **Divergence:** Flutter's `SliverFillViewport` accepts a lazy child
/// delegate (`SliverChildDelegate`); FLUI's widget is eager (all children
/// attached up-front). The geometry behaviour is identical.
///
/// # Panics
///
/// Creating or updating with a `viewport_fraction ≤ 0.0` panics inside the
/// render object.
///
/// Generic over `C: ViewSeq` of box child views.
#[derive(Clone)]
pub struct SliverFillViewport<C = Vec<BoxedView>> {
    viewport_fraction: f32,
    children: C,
}

impl<C> SliverFillViewport<C> {
    /// A fill-viewport sliver where each child occupies `viewport_fraction`
    /// of the viewport's main-axis extent.
    ///
    /// # Panics
    ///
    /// Panics when `viewport_fraction <= 0.0`.
    pub fn new(viewport_fraction: f32, children: C) -> Self {
        assert!(
            viewport_fraction > 0.0,
            "viewport_fraction must be greater than zero (got {viewport_fraction})"
        );
        Self {
            viewport_fraction,
            children,
        }
    }
}

impl<C: ViewSeq> fmt::Debug for SliverFillViewport<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverFillViewport")
            .field("viewport_fraction", &self.viewport_fraction)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for SliverFillViewport<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverFillViewport;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverFillViewport::new(self.viewport_fraction)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_viewport_fraction(self.viewport_fraction);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(SliverFillViewport);
