//! The target side of drag-and-drop: [`DropReceiver`], how a hit-test path
//! resolves to receivers, spring-loading and edge autoscroll (ADR-0211 §1, §5,
//! §8).

use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Matrix4, Offset};
use flui_platform_api::EventTime;
use flui_platform_api::data_transfer::{DropFeedback, TransferActions};

use super::view::DragView;
use crate::pointer::{
    HitTestEntry, InteractionDispatchError, InteractionDispatchHandle, LocalPayloadTarget,
    active_dispatch_handle, localize_point,
};

/// Where a drag is, as one receiver sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragPoint {
    /// The pointer's position in the window's root space.
    pub global: Offset<f64>,
    /// The same point in the receiver's own space, through the hit entry's
    /// transform: correct under ancestor transforms and scroll offsets.
    pub local: Offset<f64>,
    /// When, on the event timeline, if the input carried a time.
    pub time: Option<EventTime>,
}

/// A hover deadline after which a receiver activates (opens a folder, switches
/// a tab) while a drag stays over it, once per hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpringLoading {
    /// How long the drag must hover before [`DropReceiver::spring_loaded`] runs.
    pub delay: Duration,
}

impl SpringLoading {
    /// The delay platform conventions settle on (UIKit's `UISpringLoadedInteraction`
    /// and Finder both wait about this long).
    pub const DEFAULT_DELAY: Duration = Duration::from_millis(700);
}

impl Default for SpringLoading {
    fn default() -> Self {
        Self {
            delay: Self::DEFAULT_DELAY,
        }
    }
}

/// A receiver's answer to one autoscroll frame (ADR-0211 §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Autoscroll {
    /// Nothing to scroll here: the drag is not near an edge this receiver
    /// can move toward. The receivers outside it are asked next.
    #[default]
    Idle,
    /// The receiver scrolls toward the drag. The frame stops here, and the
    /// next frame asks again while the drag stays.
    Scrolling,
}

/// A drop target, as the drag machinery drives it.
///
/// Every method runs on the owner thread with no session borrow held, so an
/// implementation may call back into the session it is told about: cancel it,
/// start another. The machinery guarantees, per receiver and per drag, one
/// `drag_entered` first, any number of `drag_moved`, then exactly one of
/// `drag_left` or `dropped`.
pub trait DropReceiver {
    /// The drag entered the receiver. The answer is "what would you do if the
    /// user dropped now" (ADR-0038 §1).
    fn drag_entered(&self, drag: &DragView<'_>, at: DragPoint) -> DropFeedback;

    /// The drag moved over the receiver, which answers again.
    fn drag_moved(&self, drag: &DragView<'_>, at: DragPoint) -> DropFeedback;

    /// The drag left the receiver, or ended without dropping on it. Also sent
    /// to a receiver that declined the last move.
    fn drag_left(&self, drag: &DragView<'_>);

    /// The drag was dropped here: this receiver accepted the last move.
    /// `action` is the effect the drop resolved to.
    fn dropped(&self, drag: &DragView<'_>, at: DragPoint, action: TransferActions);

    /// The receiver's spring-loading policy, read when the drag enters it.
    fn spring_loading(&self) -> Option<SpringLoading> {
        None
    }

    /// The drag hovered past the [`SpringLoading`] delay. Once per hover.
    fn spring_loaded(&self, drag: &DragView<'_>) {
        let _ = drag;
    }

    /// One frame of edge autoscroll while the drag hovers at `at`, `elapsed`
    /// after the previous frame of this hover (zero when the hover carries no
    /// time yet).
    ///
    /// Each frame asks the receivers under the drag innermost first, and the
    /// first that answers [`Autoscroll::Scrolling`] ends that frame: an inner
    /// scrollable near its edge scrolls, and the ones around it do not scroll
    /// on their own as well. Frames keep coming while a receiver scrolls and
    /// resume at the drag's next move once none does.
    fn autoscroll(&self, drag: &DragView<'_>, at: DragPoint, elapsed: Duration) -> Autoscroll {
        let _ = (drag, at, elapsed);
        Autoscroll::Idle
    }
}

/// How a receiver reaches hit-test metadata: a render node is tagged with the
/// [`LocalPayloadTarget`] of a registered `DropReceiverSlot`.
///
/// The slot wraps the receiver because the lane stores `dyn Any`, and a
/// `Rc<dyn DropReceiver>` is not one.
#[derive(Clone)]
pub struct DropReceiverSlot(pub Rc<dyn DropReceiver>);

impl std::fmt::Debug for DropReceiverSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DropReceiverSlot").finish_non_exhaustive()
    }
}

/// A receiver found under the pointer.
#[derive(Clone)]
pub(crate) struct ResolvedReceiver {
    pub(super) key: LocalPayloadTarget,
    pub(super) receiver: Rc<dyn DropReceiver>,
    pub(super) local: Offset<f64>,
    /// The lane the ticket was resolved in, to ask later whether it still
    /// names this receiver.
    pub(super) lane: InteractionDispatchHandle,
}

impl ResolvedReceiver {
    /// Whether `other` is this same registration: the same ticket, holding
    /// the same receiver. A ticket whose payload was replaced names another.
    pub(super) fn is_same(&self, key: LocalPayloadTarget, receiver: &Rc<dyn DropReceiver>) -> bool {
        self.key == key && Rc::ptr_eq(&self.receiver, receiver)
    }
}

/// Whether `key` still holds `receiver` in `lane`: false once its widget
/// unregistered it or put another receiver under the ticket. A lane that
/// cannot be asked from here (no active scope) cannot tell, and keeps it.
pub(super) fn still_registered(
    lane: &InteractionDispatchHandle,
    key: LocalPayloadTarget,
    receiver: &Rc<dyn DropReceiver>,
) -> bool {
    match lane.resolve_local_payload(key) {
        Ok(payload) => payload
            .downcast::<DropReceiverSlot>()
            .is_ok_and(|slot| Rc::ptr_eq(&slot.0, receiver)),
        Err(InteractionDispatchError::TargetGone | InteractionDispatchError::OwnerGone) => false,
        Err(_) => true,
    }
}

impl std::fmt::Debug for ResolvedReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedReceiver").finish_non_exhaustive()
    }
}

/// The receivers on a hit-test `path`, innermost first.
///
/// Must run inside the owner's lane scope, where dispatch runs. A ticket that no
/// longer resolves (its receiver unmounted since the hit test) is skipped, as is
/// a node tagged with something that is not a receiver.
///
/// # Errors
///
/// Any lane error other than a gone target: the question could not be asked, which
/// is different from nothing being there.
pub(crate) fn receivers_on_path(
    path: &[HitTestEntry],
    global: Offset<f64>,
) -> Result<Vec<ResolvedReceiver>, InteractionDispatchError> {
    let mut found: Vec<ResolvedReceiver> = Vec::new();
    let mut lane: Option<InteractionDispatchHandle> = None;
    for entry in path {
        let Some(key) = entry.metadata_as::<LocalPayloadTarget>().copied() else {
            continue;
        };
        if found.iter().any(|receiver| receiver.key == key) {
            continue;
        }
        let lane = match &lane {
            Some(lane) => lane,
            None => lane.insert(active_dispatch_handle()?),
        };
        let payload = match lane.resolve_local_payload(key) {
            Ok(payload) => payload,
            Err(InteractionDispatchError::TargetGone) => continue,
            Err(error) => return Err(error),
        };
        let Ok(slot) = payload.downcast::<DropReceiverSlot>() else {
            continue;
        };
        // A receiver whose plane the drag cannot reach (behind the camera,
        // edge-on) is not under it, as a pointer target would not be.
        let Some(local) = localize(global, entry.transform.as_ref()) else {
            continue;
        };
        found.push(ResolvedReceiver {
            key,
            receiver: Rc::clone(&slot.0),
            local,
            lane: lane.clone(),
        });
    }
    Ok(found)
}

/// The receiver's own point for `global`, as pointer delivery localizes it.
fn localize(global: Offset<f64>, transform: Option<&Matrix4>) -> Option<Offset<f64>> {
    match transform {
        None => Some(global),
        Some(transform) => localize_point(global, transform),
    }
}
