//! [`SliverGrid`] — an eager 2-D grid sliver.

use std::fmt;
use std::sync::Arc;

use flui_objects::RenderSliverGrid;
use flui_rendering::delegates::SliverGridDelegate;
use flui_rendering::protocol::SliverProtocol;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// An eager sliver that arranges its box children in a 2-D grid.
///
/// Layout geometry (tile size, row count, cross-axis column positions) is
/// delegated to a [`SliverGridDelegate`]. All children are attached up-front
/// (eager); use `GridView` for the common composed form.
///
/// Mirrors Flutter's `SliverGrid` (`widgets/sliver.dart`, line 739) over
/// `RenderSliverGrid`. Lives inside a [`Viewport`](crate::Viewport).
///
/// Flutter parity: `packages/flutter/lib/src/widgets/sliver.dart` `SliverGrid`.
///
/// Generic over `C: ViewSeq` of box child views.
#[derive(Clone)]
pub struct SliverGrid<C = Vec<BoxedView>> {
    grid_delegate: Arc<dyn SliverGridDelegate>,
    children: C,
}

impl<C> SliverGrid<C> {
    /// An eager grid sliver driven by `grid_delegate` over `children`.
    pub fn new(grid_delegate: Arc<dyn SliverGridDelegate>, children: C) -> Self {
        Self {
            grid_delegate,
            children,
        }
    }
}

impl<C: ViewSeq> fmt::Debug for SliverGrid<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverGrid")
            .field("grid_delegate", &self.grid_delegate)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for SliverGrid<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverGrid;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverGrid::new(Arc::clone(&self.grid_delegate))
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_grid_delegate(Arc::clone(&self.grid_delegate));
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(SliverGrid);
