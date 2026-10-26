//! Edge autoscroll for drags (ADR-0211 §8).
//!
//! Every [`Scrollable`](super::Scrollable) is a drop receiver that takes no
//! drop. While a drag hovers within the band along one of its edges, each
//! frame steps the scrollable toward that edge as a
//! [`ScrollCause::SideEffect`](super::ScrollCause::SideEffect) delta of the
//! drag's own contactless sequence, through the nested scroll channel
//! (ADR-0209): participants above see it, and what the leaf cannot take goes
//! up the chain like any other remainder. The drag machinery asks the
//! receivers under a drag innermost first and stops at the first that
//! scrolls, so a list inside a list scrolls only once.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::{Axis, Vec2};
use flui_interaction::LocalPayloadTarget;
use flui_interaction::drag::{Autoscroll, DragPoint, DragView, DropReceiver, DropReceiverSlot};
use flui_objects::RenderMetaData;
use flui_platform::data_transfer::{DropFeedback, TransferActions};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, RenderObjectContext, RenderView, impl_render_view};

use super::nested::ScrollDevice;
use super::scroll_owner::ScrollOwner;

/// The band along each edge a hovering drag scrolls in, in logical pixels;
/// a quarter of a viewport too small to hold two full bands.
const EDGE_BAND: f64 = 48.0;

/// How fast a drag at the very edge scrolls, in logical pixels per second.
/// The speed falls linearly to nothing at the inner side of the band, so the
/// user controls it by how far into the band they hold the drag.
const EDGE_SPEED: f64 = 1200.0;

/// The longest frame one step covers: a stalled frame resumes at speed
/// instead of jumping.
const MAX_FRAME: Duration = Duration::from_millis(100);

/// The speed along the axis, in screen direction, of a drag at `position`
/// within a viewport `extent` long: negative toward the start edge, positive
/// toward the end edge, zero outside both bands.
fn edge_speed(position: f64, extent: f64) -> f64 {
    if !position.is_finite() || !extent.is_finite() || extent <= 0.0 {
        return 0.0;
    }
    let band = EDGE_BAND.min(extent / 4.0);
    let toward = |distance: f64| EDGE_SPEED * (1.0 - distance.max(0.0) / band);
    if position < band {
        -toward(position)
    } else if extent - position < band {
        toward(extent - position)
    } else {
        0.0
    }
}

/// A scrollable's drop receiver: the state's owner cell, read at each call
/// so the current configuration's owner scrolls.
pub(crate) struct DragAutoscroll {
    owner: Rc<RefCell<Option<Rc<ScrollOwner>>>>,
}

impl DragAutoscroll {
    pub(crate) fn new(owner: Rc<RefCell<Option<Rc<ScrollOwner>>>>) -> Self {
        Self { owner }
    }

    fn owner(&self) -> Option<Rc<ScrollOwner>> {
        self.owner.borrow().clone()
    }

    fn stop(&self, drag: &DragView<'_>) {
        if let (Some(owner), Some(hovering)) = (self.owner(), drag.identity()) {
            owner.end_autoscroll(hovering);
        }
    }
}

impl DropReceiver for DragAutoscroll {
    fn drag_entered(&self, _drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        DropFeedback { accept: None }
    }

    fn drag_moved(&self, _drag: &DragView<'_>, _at: DragPoint) -> DropFeedback {
        DropFeedback { accept: None }
    }

    fn drag_left(&self, drag: &DragView<'_>) {
        self.stop(drag);
    }

    fn dropped(&self, drag: &DragView<'_>, _at: DragPoint, _action: TransferActions) {
        self.stop(drag);
    }

    fn autoscroll(&self, drag: &DragView<'_>, at: DragPoint, elapsed: Duration) -> Autoscroll {
        let (Some(owner), Some(hovering)) = (self.owner(), drag.identity()) else {
            return Autoscroll::Idle;
        };
        let extent = owner.controller().position().viewport_dimension();
        let speed = match owner.axis() {
            Axis::Vertical => edge_speed(at.local.dy, extent),
            Axis::Horizontal => edge_speed(at.local.dx, extent),
        };
        if speed == 0.0 {
            owner.end_autoscroll(hovering);
            return Autoscroll::Idle;
        }
        let velocity = match owner.axis() {
            Axis::Vertical => Vec2::new(0.0, speed),
            Axis::Horizontal => Vec2::new(speed, 0.0),
        };
        let device = ScrollDevice::Pointer {
            kind: drag.pointer_kind(),
            device: None,
        };
        if owner.autoscroll(hovering, device, velocity, elapsed.min(MAX_FRAME)) {
            Autoscroll::Scrolling
        } else {
            Autoscroll::Idle
        }
    }
}

/// Registers a scrollable's [`DragAutoscroll`] in the owner lane and tags the
/// viewport's place in the render tree with the ticket, so a drag over the
/// viewport finds it. Found whenever the scrollable's own gesture area is.
#[derive(Clone)]
pub(crate) struct DragAutoscrollAnchor {
    receiver: Rc<DragAutoscroll>,
    child: Child,
}

impl DragAutoscrollAnchor {
    pub(crate) fn new(receiver: Rc<DragAutoscroll>, child: Child) -> Self {
        Self { receiver, child }
    }

    fn publish(&self, ctx: &RenderObjectContext<'_>, render_object: &mut RenderMetaData) {
        let slot: Rc<dyn Any> = Rc::new(DropReceiverSlot(
            Rc::clone(&self.receiver) as Rc<dyn DropReceiver>
        ));
        match ctx.register_local_payload(slot) {
            Ok(target) => {
                render_object.set_shared_metadata(Some(Arc::new(target)));
            }
            Err(error) => tracing::debug!(
                ?error,
                "scrollable mounted without an owner lane; drags do not autoscroll it yet"
            ),
        }
    }
}

impl std::fmt::Debug for DragAutoscrollAnchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragAutoscrollAnchor")
            .finish_non_exhaustive()
    }
}

impl RenderView for DragAutoscrollAnchor {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMetaData;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderMetaData::new();
        render_object.set_behavior(HitTestBehavior::DeferToChild);
        self.publish(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The receiver is the state's own, the same across rebuilds, so an
        // existing ticket still resolves to it.
        if render_object.metadata_as::<LocalPayloadTarget>().is_none() {
            self.publish(ctx, render_object);
        }
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn did_unmount_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.metadata_as::<LocalPayloadTarget>().copied()
            && let Err(error) = ctx.unregister_local_payload(target)
        {
            tracing::debug!(?error, "scrollable drop receiver was already unregistered");
        }
        render_object.clear_metadata();
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(DragAutoscrollAnchor);
