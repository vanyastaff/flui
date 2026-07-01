//! [`ListBody`] — sequential multi-child body layout.

use std::fmt;

use flui_objects::RenderListBody;
use flui_rendering::protocol::BoxProtocol;
use flui_types::layout::{Axis, AxisDirection};
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Lays children out sequentially along one axis, stretching them in the cross
/// axis.
///
/// Flutter parity: `widgets/basic.dart` `ListBody` over `RenderListBody`.
/// `ListBody` expects its parent to provide unbounded space along the main axis
/// and a bounded cross axis, typically inside a matching scrollable.
#[derive(Clone)]
pub struct ListBody<C = Vec<BoxedView>> {
    main_axis: Axis,
    reverse: bool,
    children: C,
}

impl<C> ListBody<C> {
    /// Creates a vertical top-to-bottom list body with the given children.
    pub fn new(children: C) -> Self {
        Self {
            main_axis: Axis::Vertical,
            reverse: false,
            children,
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

    fn axis_direction(&self) -> AxisDirection {
        AxisDirection::from_axis(self.main_axis, self.reverse)
    }
}

impl<C: ViewSeq> fmt::Debug for ListBody<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListBody")
            .field("main_axis", &self.main_axis)
            .field("reverse", &self.reverse)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for ListBody<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderListBody;

    fn create_render_object(&self) -> RenderListBody {
        RenderListBody::with_axis_direction(self.axis_direction())
    }

    fn update_render_object(&self, render_object: &mut RenderListBody) {
        render_object.set_axis_direction(self.axis_direction());
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(ListBody);
