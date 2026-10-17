//! The target side of drag-and-drop: [`DropReceiver`], how a hit-test path
//! resolves to receivers, and spring-loading (ADR-0211 §1, §5, §8).

use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{Matrix4, Offset};
use flui_platform_api::EventTime;
use flui_platform_api::data_transfer::{DropFeedback, TransferActions};

use super::view::DragView;
use crate::pointer::{
    HitTestEntry, InteractionDispatchError, LocalPayloadTarget, resolve_local_payload,
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
pub struct ResolvedReceiver {
    pub(super) key: LocalPayloadTarget,
    pub(super) receiver: Rc<dyn DropReceiver>,
    pub(super) local: Offset<f64>,
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
pub fn receivers_on_path(
    path: &[HitTestEntry],
    global: Offset<f64>,
) -> Result<Vec<ResolvedReceiver>, InteractionDispatchError> {
    let mut found: Vec<ResolvedReceiver> = Vec::new();
    for entry in path {
        let Some(key) = entry.metadata_as::<LocalPayloadTarget>().copied() else {
            continue;
        };
        if found.iter().any(|receiver| receiver.key == key) {
            continue;
        }
        let payload = match resolve_local_payload(key) {
            Ok(payload) => payload,
            Err(InteractionDispatchError::TargetGone) => continue,
            Err(error) => return Err(error),
        };
        let Ok(slot) = payload.downcast::<DropReceiverSlot>() else {
            continue;
        };
        found.push(ResolvedReceiver {
            key,
            receiver: Rc::clone(&slot.0),
            local: localize(global, entry.transform.as_ref()),
        });
    }
    Ok(found)
}

fn localize(global: Offset<f64>, transform: Option<&Matrix4>) -> Offset<f64> {
    let Some(transform) = transform else {
        return global;
    };
    let (x, y) = transform.transform_point(global.dx, global.dy);
    Offset::new(x, y)
}
