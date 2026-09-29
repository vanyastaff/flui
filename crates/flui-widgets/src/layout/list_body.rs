//! [`ListBody`] — sequential multi-child body layout.

use std::fmt;

use flui_foundation::geometry::Axis;
use flui_objects::RenderListBody;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::protocol::BoxProtocol;
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
        // Only a HORIZONTAL list body can use a reading direction; the
        // vertical arm of `resolve_axis_direction` discards it. Looking it up
        // regardless would register an inherited dependency the widget cannot
        // act on, so every vertical `ListBody` would rebuild on a direction
        // change that cannot move it. The reference keeps the lookup inside
        // the horizontal case for the same reason
        // (`getAxisDirectionFromAxisReverseAndDirectionality`,
        // `widgets/basic.dart:4513-4527`).
        let text_direction = match self.main_axis {
            Axis::Horizontal => Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr),
            Axis::Vertical => TextDirection::Ltr,
        };
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
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_axis_direction(self.axis_direction);
        impact
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(ListBodyRenderView);
