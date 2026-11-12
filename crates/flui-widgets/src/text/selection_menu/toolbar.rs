//! The toolbar a touch shows over a selection, as a field or an area puts it
//! in the overlay beside its handles: laid out in the overlay's space over
//! the selection's start, or under its end where there is no room above,
//! and pinned to the start's leader so it moves with the selection.

use std::rc::Rc;

use flui_foundation::geometry::{Offset, Point, Size};
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::delegates::SingleChildLayoutDelegate;
use flui_rendering::layer::LayerLink;
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::{FrameTiming, PostFrameHandle};
use flui_view::prelude::*;
use flui_view::{BoxedView, View, ViewExt};

use super::SelectionMenu;
use super::host::MenuHost;
use crate::interaction::MenuItem;
use crate::localization::Directionality;
use crate::overlay::OverlayHandle;
use crate::text::selection_area::handles::LinkFollower;
use crate::{CustomSingleChildLayout, Positioned, SizedBox};

/// How far the toolbar stands off the line it goes over or under, and off
/// the overlay's edges.
const TOOLBAR_GAP: f64 = 8.0;
/// How far under the line a toolbar flipped below it stands, clear of the
/// handle there.
const TOOLBAR_BELOW_GAP: f64 = 32.0;

/// Where a toolbar goes, in the overlay's space: over `over` (the top
/// middle of the start's line), else under `under` (the bottom middle of
/// the end's line), inside a room the overlay's size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToolbarAnchors {
    over: Offset<f64>,
    under: Offset<f64>,
    room: Size<f64>,
}

impl ToolbarAnchors {
    /// The anchors of a selection whose start's line has its middle at
    /// `start` and is `start_height` tall, and whose end's line has its
    /// middle at `end` and is `end_height` tall, all in render-root
    /// coordinates, moved into `overlay`'s space. `None` while the overlay
    /// is not laid out.
    pub(crate) fn in_overlay(
        overlay: &OverlayHandle,
        pipeline: &PipelineCell,
        (start, start_height): (Offset<f64>, f64),
        (end, end_height): (Offset<f64>, f64),
    ) -> Option<Self> {
        let overlay = overlay.render_id()?;
        let points = [
            Offset::new(start.dx, start.dy - start_height / 2.0),
            Offset::new(end.dx, end.dy + end_height / 2.0),
        ];
        pipeline
            .try_with_layout(|owner| {
                let room = owner.box_size(overlay)?;
                let mut moved = points;
                for point in &mut moved {
                    let local =
                        owner.global_to_local(overlay, Point::new(point.dx, point.dy), None)?;
                    *point = Offset::new(local.x, local.y);
                }
                let [over, under] = moved;
                Some(Self { over, under, room })
            })
            .flatten()
    }
}

/// The host's toolbar offering the items it can perform now, placed by
/// `anchors` and pinned to `start`, the leader of the selection's start
/// (the caret while a caret's handle shows), so it moves with the
/// selection (a scroll) and goes while the start is clipped out. Choosing
/// an item, by a touch or by assistive technology, runs `hide` after the
/// frame, once the item has performed. `None` without a look or anything
/// to offer.
pub(crate) fn toolbar(
    host: &MenuHost,
    anchors: ToolbarAnchors,
    start: LayerLink,
    post_frame: Option<PostFrameHandle>,
    hide: Rc<dyn Fn()>,
) -> Option<BoxedView> {
    let look = host.look()?;
    let offered = host.offered();
    if offered.is_empty() {
        return None;
    }
    let chosen: Rc<dyn Fn()> = Rc::new(move || {
        let hide = Rc::clone(&hide);
        if let Some(post_frame) = &post_frame {
            let _ = post_frame.schedule(move |_timing: &FrameTiming| hide());
        }
    });
    let items = offered
        .into_iter()
        .map(|item| item.when_chosen(Rc::clone(&chosen)))
        .collect::<Rc<[MenuItem]>>();
    // The look builds under the owner's reading direction, though the
    // toolbar stands in the overlay.
    let bar = Directionality::new(host.direction(), ToolbarLook { look, items });
    let ToolbarAnchors { over, under, room } = anchors;
    let layout = CustomSingleChildLayout::new(ToolbarLayout { over, under }).child(bar);
    let room = SizedBox::new(room.width, room.height).child(layout);
    let pinned = LinkFollower::pinned(start, Offset::new(-over.dx, -over.dy), room);
    Some(Positioned::new(pinned).left(0.0).top(0.0).boxed())
}

/// The menu's toolbar as its look draws it, built where it stands so the
/// look reads the context above it.
#[derive(Clone)]
struct ToolbarLook {
    look: Rc<dyn SelectionMenu>,
    items: Rc<[MenuItem]>,
}

impl std::fmt::Debug for ToolbarLook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolbarLook")
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

impl View for ToolbarLook {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for ToolbarLook {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.look.toolbar(&self.items, ctx)
    }
}

/// Places the toolbar in the overlay: its bottom centre [`TOOLBAR_GAP`] over
/// `over`, else its top centre [`TOOLBAR_BELOW_GAP`] under `under`, then
/// moved sideways and down into the overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ToolbarLayout {
    over: Offset<f64>,
    under: Offset<f64>,
}

impl SingleChildLayoutDelegate for ToolbarLayout {
    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        let loose = constraints.loosen();
        // The toolbar keeps a gap from both edges.
        let width = (loose.max_width - 2.0 * TOOLBAR_GAP).max(0.0);
        BoxConstraints::new(0.0, width, 0.0, loose.max_height)
    }

    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset<f64> {
        let top = self.over.dy - TOOLBAR_GAP - child_size.height;
        let (x, y) = if top >= TOOLBAR_GAP {
            (self.over.dx, top)
        } else {
            (self.under.dx, self.under.dy + TOOLBAR_BELOW_GAP)
        };
        let clamp = |at: f64, extent: f64, room: f64| {
            let max = (room - extent - TOOLBAR_GAP).max(TOOLBAR_GAP);
            if at.is_finite() {
                at.clamp(TOOLBAR_GAP, max)
            } else {
                TOOLBAR_GAP
            }
        };
        Offset::new(
            clamp(x - child_size.width / 2.0, child_size.width, size.width),
            clamp(y, child_size.height, size.height),
        )
    }

    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old != self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
