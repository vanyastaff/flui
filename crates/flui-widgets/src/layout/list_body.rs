//! [`ListBody`] — sequential multi-child body layout.

use std::fmt;

use flui_foundation::geometry::Axis;
use flui_objects::RenderListBody;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, CreationSite};

use crate::localization::{Directionality, resolve_horizontal_axis_direction};
use crate::support::generic_render_view_element;

/// Lays children out sequentially along one axis, stretching them in the cross
/// axis.
///
/// Backed by `RenderListBody`. `ListBody` expects its parent to provide
/// unbounded space along the main axis and a bounded cross axis, typically
/// inside a matching scrollable.
///
/// Resolves its `AxisDirection` from the axis, `reverse` and the ambient
/// text direction: a horizontal `ListBody` reads the ambient [`Directionality`]
/// and picks `LeftToRight`/`RightToLeft` accordingly (defaulting to `Ltr` with
/// no ancestor, matching every other FLUI widget that consults
/// `Directionality`), then `reverse` flips the result to its opposite. A
/// vertical `ListBody` never consults `Directionality` — `reverse` alone
/// decides `TopToBottom` vs. `BottomToTop`. The read goes through
/// [`RenderObjectContext::depend_on`](flui_view::RenderObjectContext::depend_on),
/// so the render object re-pushes its configuration when the direction changes.
#[derive(Clone)]
pub struct ListBody<C = Vec<BoxedView>> {
    main_axis: Axis,
    reverse: bool,
    children: C,
    site: CreationSite,
}

impl<C> ListBody<C> {
    /// Creates a vertical top-to-bottom list body with the given children.
    #[track_caller]
    pub fn new(children: C) -> Self {
        Self {
            main_axis: Axis::Vertical,
            reverse: false,
            children,
            site: CreationSite::here(),
        }
    }

    /// The main axis along which children are placed.
    #[must_use]
    pub fn main_axis(mut self, axis: Axis) -> Self {
        self.main_axis = axis;
        self
    }

    /// Whether children are placed in the reverse direction for the main axis.
    #[must_use]
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Resolve the render object's `AxisDirection`: only `Axis::Horizontal`
    /// consults the ambient direction, so a vertical body registers no
    /// `Directionality` dependency; `reverse` flips either axis's base
    /// direction to its opposite.
    fn resolve_axis_direction(&self, ctx: &flui_view::RenderObjectContext<'_>) -> AxisDirection {
        match self.main_axis {
            Axis::Horizontal => resolve_horizontal_axis_direction(
                Directionality::of_render_object(ctx),
                self.reverse,
            ),
            Axis::Vertical => AxisDirection::from_axis(Axis::Vertical, self.reverse),
        }
    }
}

impl<C: ViewSeq> fmt::Debug for ListBody<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListBody")
            .field("main_axis", &self.main_axis)
            .field("reverse", &self.reverse)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<C> flui_view::RenderView for ListBody<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderListBody;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> RenderListBody {
        RenderListBody::with_axis_direction(self.resolve_axis_direction(ctx))
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderListBody,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_axis_direction(self.resolve_axis_direction(ctx))
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(ListBody, site = site);
