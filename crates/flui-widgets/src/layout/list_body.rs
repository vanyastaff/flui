//! [`ListBody`] — sequential multi-child body layout.

use std::fmt;

use flui_objects::RenderListBody;
use flui_rendering::protocol::BoxProtocol;
use flui_types::layout::{Axis, AxisDirection};
use flui_types::typography::TextDirection;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::localization::Directionality;
use crate::support::generic_render_view_element;

/// Lays children out sequentially along one axis, stretching them in the cross
/// axis.
///
/// Flutter parity: `widgets/basic.dart` `ListBody` over `RenderListBody`.
/// `ListBody` expects its parent to provide unbounded space along the main axis
/// and a bounded cross axis, typically inside a matching scrollable.
///
/// Resolves its `AxisDirection` the way Flutter's
/// `getAxisDirectionFromAxisReverseAndDirectionality` does (`widgets/
/// basic.dart`): a horizontal `ListBody` reads the ambient [`Directionality`]
/// and picks `LeftToRight`/`RightToLeft` accordingly (defaulting to `Ltr` with
/// no ancestor, matching every other FLUI widget that consults
/// `Directionality`), then `reverse` flips the result to its opposite. A
/// vertical `ListBody` never consults `Directionality` — `reverse` alone
/// decides `TopToBottom` vs. `BottomToTop`, mirroring `_getDirection`'s own
/// `Axis.vertical` arm. That ambient read only exists inside a
/// `BuildContext`, so `ListBody` is a composing widget: `build` resolves the
/// direction once and hands the *already-resolved* `AxisDirection` to a
/// private render-object widget — `flui_view::RenderObjectContext` (the only
/// context `RenderView::create_render_object`/`update_render_object` ever
/// receive) carries no ambient-lookup capability, so a bare `RenderView` can
/// never read `Directionality` itself.
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

    /// Resolve the render object's `AxisDirection`: Flutter's
    /// `getAxisDirectionFromAxisReverseAndDirectionality` (`widgets/
    /// basic.dart`) — only `Axis::Horizontal` consults `text_direction`;
    /// `reverse` flips either axis's base direction to its opposite.
    fn resolve_axis_direction(&self, text_direction: TextDirection) -> AxisDirection {
        match self.main_axis {
            Axis::Horizontal => {
                let base = if text_direction.is_rtl() {
                    AxisDirection::RightToLeft
                } else {
                    AxisDirection::LeftToRight
                };
                if self.reverse { base.opposite() } else { base }
            }
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
            .finish()
    }
}

impl<C> flui_view::View for ListBody<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<C> flui_view::StatelessView for ListBody<C>
where
    C: ViewSeq + Clone + 'static,
{
    fn build(&self, ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
        ListBodyRenderView {
            axis_direction: self.resolve_axis_direction(text_direction),
            children: self.children.clone(),
        }
    }
}

/// The actual `RenderListBody`-backed render-object widget. Its
/// `AxisDirection` is already resolved by [`ListBody`]'s `StatelessView::build`
/// — kept private
/// so `Directionality` is only ever read at the `BuildContext` seam that can
/// see it, never assumed to be reachable from a bare `RenderView`.
#[derive(Clone)]
struct ListBodyRenderView<C> {
    axis_direction: AxisDirection,
    children: C,
}

impl<C> flui_view::RenderView for ListBodyRenderView<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderListBody;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> RenderListBody {
        RenderListBody::with_axis_direction(self.axis_direction)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderListBody,
    ) {
        render_object.set_axis_direction(self.axis_direction);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(ListBodyRenderView);

#[cfg(test)]
mod tests {
    use flui_types::layout::AxisDirection;
    use flui_view::RenderView;
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;

    // ------------------------------------------------------------------
    // `resolve_axis_direction` — the Flutter parity rule under test. Covers
    // every (axis, reverse, text_direction) combination `getAxisDirectionFrom
    // AxisReverseAndDirectionality` handles, including the RTL cases a
    // vertical-only `AxisDirection::from_axis` call could never reach.
    // ------------------------------------------------------------------

    #[test]
    fn resolve_axis_direction_vertical_defaults_to_top_to_bottom() {
        let list_body: ListBody = ListBody::new(Vec::new());
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Ltr),
            AxisDirection::TopToBottom
        );
    }

    #[test]
    fn resolve_axis_direction_vertical_reverse_is_bottom_to_top() {
        let list_body: ListBody = ListBody::new(Vec::new()).reverse(true);
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Ltr),
            AxisDirection::BottomToTop
        );
    }

    /// The vertical axis never consults `text_direction` — Flutter's
    /// `_getDirection`'s `Axis.vertical` arm ignores it too.
    #[test]
    fn resolve_axis_direction_vertical_ignores_text_direction() {
        let list_body: ListBody = ListBody::new(Vec::new()).reverse(true);
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Rtl),
            AxisDirection::BottomToTop,
            "an RTL ambient direction must not change a vertical ListBody's axis direction"
        );
    }

    #[test]
    fn resolve_axis_direction_horizontal_ltr_is_left_to_right() {
        let list_body: ListBody = ListBody::new(Vec::new()).main_axis(Axis::Horizontal);
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Ltr),
            AxisDirection::LeftToRight
        );
    }

    #[test]
    fn resolve_axis_direction_horizontal_rtl_is_right_to_left() {
        let list_body: ListBody = ListBody::new(Vec::new()).main_axis(Axis::Horizontal);
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Rtl),
            AxisDirection::RightToLeft,
            "a horizontal ListBody under RTL Directionality must lay out right to left"
        );
    }

    #[test]
    fn resolve_axis_direction_horizontal_ltr_reverse_is_right_to_left() {
        let list_body: ListBody = ListBody::new(Vec::new())
            .main_axis(Axis::Horizontal)
            .reverse(true);
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Ltr),
            AxisDirection::RightToLeft
        );
    }

    /// `reverse` flips the RTL base direction too: RTL + reverse ends up back
    /// at `LeftToRight`, matching `flipAxisDirection(AxisDirection.left)`.
    #[test]
    fn resolve_axis_direction_horizontal_rtl_reverse_is_left_to_right() {
        let list_body: ListBody = ListBody::new(Vec::new())
            .main_axis(Axis::Horizontal)
            .reverse(true);
        assert_eq!(
            list_body.resolve_axis_direction(TextDirection::Rtl),
            AxisDirection::LeftToRight
        );
    }

    // ------------------------------------------------------------------
    // `ListBodyRenderView` — the private render-object widget `ListBody::
    // build` delegates to once the direction is resolved. Verifies the
    // resolved `AxisDirection` (and children) actually reach `RenderListBody`
    // and can be updated, independent of the ambient-direction resolution
    // above.
    // ------------------------------------------------------------------

    #[test]
    fn render_view_create_render_object_uses_the_resolved_direction() {
        let render_view = ListBodyRenderView {
            axis_direction: AxisDirection::RightToLeft,
            children: Vec::<BoxedView>::new(),
        };
        let render_object =
            render_view.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.axis_direction(), AxisDirection::RightToLeft);
    }

    #[test]
    fn render_view_update_render_object_applies_a_changed_direction() {
        let render_view = ListBodyRenderView {
            axis_direction: AxisDirection::TopToBottom,
            children: Vec::<BoxedView>::new(),
        };
        let mut render_object =
            render_view.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.axis_direction(), AxisDirection::TopToBottom);

        let updated = ListBodyRenderView {
            axis_direction: AxisDirection::RightToLeft,
            children: Vec::<BoxedView>::new(),
        };
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.axis_direction(), AxisDirection::RightToLeft);
    }

    #[test]
    fn render_view_has_children_reflects_an_empty_child_list() {
        let empty = ListBodyRenderView {
            axis_direction: AxisDirection::TopToBottom,
            children: Vec::<BoxedView>::new(),
        };
        assert!(!empty.has_children());

        let non_empty = ListBodyRenderView {
            axis_direction: AxisDirection::TopToBottom,
            children: vec![SizedBox::shrink().boxed()],
        };
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
    fn render_view_visit_child_views_invokes_the_visitor_once_per_child() {
        let render_view = ListBodyRenderView {
            axis_direction: AxisDirection::TopToBottom,
            children: vec![SizedBox::shrink().boxed(), SizedBox::shrink().boxed()],
        };
        let mut visited = 0;
        render_view.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 2, "visitor must run once per child");
    }

    #[test]
    fn render_view_visit_child_views_does_not_invoke_the_visitor_without_children() {
        let empty = ListBodyRenderView {
            axis_direction: AxisDirection::TopToBottom,
            children: Vec::<BoxedView>::new(),
        };
        let mut visited = 0;
        empty.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 0, "no children -> visitor must not run");
    }
}
