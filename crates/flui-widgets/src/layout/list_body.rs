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

#[cfg(test)]
mod tests {
    use flui_types::layout::AxisDirection;
    use flui_view::RenderView;
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_defaults_to_top_to_bottom() {
        let list_body: ListBody = ListBody::new(Vec::new());
        let render_object = list_body.create_render_object();
        assert_eq!(render_object.axis_direction(), AxisDirection::TopToBottom);
    }

    #[test]
    fn create_render_object_reverse_vertical_is_bottom_to_top() {
        let list_body: ListBody = ListBody::new(Vec::new()).reverse(true);
        let render_object = list_body.create_render_object();
        assert_eq!(render_object.axis_direction(), AxisDirection::BottomToTop);
    }

    #[test]
    fn create_render_object_horizontal_is_left_to_right() {
        let list_body: ListBody = ListBody::new(Vec::new()).main_axis(Axis::Horizontal);
        let render_object = list_body.create_render_object();
        assert_eq!(render_object.axis_direction(), AxisDirection::LeftToRight);
    }

    #[test]
    fn create_render_object_horizontal_reverse_is_right_to_left() {
        let list_body: ListBody = ListBody::new(Vec::new())
            .main_axis(Axis::Horizontal)
            .reverse(true);
        let render_object = list_body.create_render_object();
        assert_eq!(render_object.axis_direction(), AxisDirection::RightToLeft);
    }

    #[test]
    fn update_render_object_applies_a_changed_axis_direction() {
        let list_body: ListBody = ListBody::new(Vec::new());
        let mut render_object = list_body.create_render_object();
        assert_eq!(render_object.axis_direction(), AxisDirection::TopToBottom);

        let updated: ListBody = ListBody::new(Vec::new())
            .main_axis(Axis::Horizontal)
            .reverse(true);
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.axis_direction(), AxisDirection::RightToLeft);
    }

    #[test]
    fn has_children_reflects_an_empty_child_list() {
        let empty: ListBody = ListBody::new(Vec::new());
        assert!(!empty.has_children());

        let non_empty = ListBody::new(vec![SizedBox::shrink().boxed()]);
        assert!(non_empty.has_children());
    }

    #[test]
    fn debug_reports_defaults_and_child_count() {
        let list_body = ListBody::new(vec![SizedBox::shrink().boxed()]);
        let debug = format!("{list_body:?}");
        assert!(
            debug.contains("main_axis: Vertical")
                && debug.contains("reverse: false")
                && debug.contains("children: 1"),
            "Debug output must report main_axis, reverse and children count, got: {debug}",
        );
    }

    #[test]
    fn debug_reports_overridden_main_axis_and_reverse() {
        let list_body: ListBody = ListBody::new(Vec::new())
            .main_axis(Axis::Horizontal)
            .reverse(true);
        let debug = format!("{list_body:?}");
        assert!(
            debug.contains("main_axis: Horizontal")
                && debug.contains("reverse: true")
                && debug.contains("children: 0"),
            "Debug output must report the overridden main_axis and reverse, got: {debug}",
        );
    }

    #[test]
    fn visit_child_views_invokes_the_visitor_once_per_child() {
        let list_body = ListBody::new(vec![SizedBox::shrink().boxed(), SizedBox::shrink().boxed()]);
        let mut visited = 0;
        list_body.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 2, "visitor must run once per child");
    }

    #[test]
    fn visit_child_views_does_not_invoke_the_visitor_without_children() {
        let empty: ListBody = ListBody::new(Vec::new());
        let mut visited = 0;
        empty.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 0, "no children -> visitor must not run");
    }
}
