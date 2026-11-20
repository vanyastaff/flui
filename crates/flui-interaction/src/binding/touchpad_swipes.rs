//! Open touchpad swipes: the hit path each one took at its start.
//!
//! A swipe is a gesture, not a scroll: its `Start` is hit-tested once where
//! the pointer rests, and every later phase is delivered along that path,
//! wherever the content under the pointer moves meanwhile. The arena keys its
//! contest by the swipe's pointer, so the recognizers its `Start` reached
//! compete for it as they would for a contact.

use crate::containment::keep_first;
use std::cell::RefCell;

use flui_foundation::geometry::Offset;
use flui_platform::input::pointer::{
    CancelReason, DeviceId, TouchpadSwipeEvent, TouchpadSwipePhase,
};
use smallvec::SmallVec;

use super::{GestureBinding, pointer_sequences::MAX_SIMULTANEOUS_POINTERS};
use crate::{
    containment::{FirstPanic, caught},
    events::{PointerEvent, PointerEventExt as _},
    gesture::{ContestCancel, arena::GestureArena},
    ids::{InputSurface, PointerId},
    pointer::HitTestResult,
};

/// One open swipe.
pub(super) struct OpenSwipe {
    pub(super) surface: InputSurface,
    /// The last event delivered, from which a cancel the binding owes is built.
    pub(super) last: TouchpadSwipeEvent,
    pub(super) path: HitTestResult,
}

impl OpenSwipe {
    pub(super) fn pointer(&self) -> PointerId {
        self.last.pointer().id
    }
}

/// The swipes open on every surface, at most [`MAX_SIMULTANEOUS_POINTERS`].
#[derive(Default)]
pub(super) struct TouchpadSwipes {
    open: RefCell<SmallVec<[OpenSwipe; 1]>>,
}

impl TouchpadSwipes {
    /// Open `swipe` along `path`. Returns the swipes it displaces: an earlier
    /// one of the same pointer on the surface that never ended, and the
    /// oldest one once the bound is reached. The caller ends each.
    pub(super) fn open(
        &self,
        surface: InputSurface,
        swipe: TouchpadSwipeEvent,
        path: HitTestResult,
    ) -> SmallVec<[OpenSwipe; 1]> {
        let mut open = self.open.borrow_mut();
        let pointer = swipe.pointer().id;
        let mut displaced = SmallVec::new();
        if let Some(index) = open
            .iter()
            .position(|held| held.surface == surface && held.pointer() == pointer)
        {
            displaced.push(open.remove(index));
        }
        if open.len() >= MAX_SIMULTANEOUS_POINTERS {
            displaced.push(open.remove(0));
        }
        open.push(OpenSwipe {
            surface,
            last: swipe,
            path,
        });
        displaced
    }

    /// The path of `swipe`'s open sequence on `surface`, recording `swipe` as
    /// its last event; `None` when it has none.
    pub(super) fn follow(
        &self,
        surface: InputSurface,
        swipe: TouchpadSwipeEvent,
    ) -> Option<HitTestResult> {
        let mut open = self.open.borrow_mut();
        let held = open
            .iter_mut()
            .find(|held| held.surface == surface && held.pointer() == swipe.pointer().id)?;
        held.last = swipe;
        Some(held.path.clone())
    }

    /// Close `swipe`'s open sequence on `surface`, returning it.
    pub(super) fn close(
        &self,
        surface: InputSurface,
        swipe: &TouchpadSwipeEvent,
    ) -> Option<OpenSwipe> {
        let mut open = self.open.borrow_mut();
        let index = open
            .iter()
            .position(|held| held.surface == surface && held.pointer() == swipe.pointer().id)?;
        Some(open.remove(index))
    }

    /// Close every open swipe of `device`, returning them oldest first.
    pub(super) fn close_device(&self, device: DeviceId) -> SmallVec<[OpenSwipe; 1]> {
        let mut open = self.open.borrow_mut();
        let mut closed = SmallVec::new();
        let mut index = 0;
        while index < open.len() {
            if open[index].last.pointer().device == Some(device) {
                closed.push(open.remove(index));
            } else {
                index += 1;
            }
        }
        closed
    }

    /// Close every open swipe, returning them oldest first.
    pub(super) fn drain(&self) -> SmallVec<[OpenSwipe; 1]> {
        std::mem::take(&mut *self.open.borrow_mut())
    }
}

impl GestureBinding {
    /// Deliver one event of a touchpad swipe along the path its `Start` took,
    /// and drive the arena contest keyed by its pointer: `Start` closes it
    /// once every recognizer on the path had the chance to join, `End` sweeps
    /// it, `Cancelled` abandons it. A phase of a swipe with no open sequence
    /// reaches no one.
    pub(super) fn handle_touchpad_swipe<F>(
        &self,
        surface: InputSurface,
        event: &PointerEvent,
        swipe: &TouchpadSwipeEvent,
        hit_test_fn: F,
    ) where
        F: FnOnce(Offset<f64>) -> HitTestResult,
    {
        let pointer = swipe.pointer().id;
        let arena = self.arena.arena();
        let mut first_panic = FirstPanic::new();
        match swipe.phase {
            TouchpadSwipePhase::Start => {
                let position = event
                    .position()
                    .expect("BUG: TouchpadSwipe carries a checked position");
                let path = hit_test_fn(position);
                // The latch commits before any user code: a recognizer that
                // re-enters with the next phase finds the sequence open.
                let displaced = self.touchpad_swipes.open(surface, *swipe, path.clone());
                for earlier in displaced {
                    self.cancel_touchpad_swipe(earlier, &mut first_panic);
                }
                let delivered = self.dispatch_ephemeral(event, &path);
                keep_first(&mut first_panic, delivered, "touchpad swipe start");
                let closed = caught(|| arena.close(pointer));
                keep_first(&mut first_panic, closed, "touchpad swipe arena close");
            }
            TouchpadSwipePhase::Update(_) => {
                if let Some(path) = self.touchpad_swipes.follow(surface, *swipe) {
                    first_panic = self.dispatch_ephemeral(event, &path);
                }
            }
            TouchpadSwipePhase::End | TouchpadSwipePhase::Cancelled => {
                let Some(open) = self.touchpad_swipes.close(surface, swipe) else {
                    return;
                };
                // The swipe's own contest leaves the maps before user code
                // runs, so a contest a callback opens for the same pointer
                // (a newer start, a press reusing the id) is not the one
                // settled here.
                let contest = self.arena.detach(pointer);
                first_panic = self.dispatch_ephemeral(event, &open.path);
                let settled = caught(|| {
                    if swipe.phase == TouchpadSwipePhase::End {
                        arena.sweep_detached(contest);
                    } else {
                        GestureArena::abandon_detached(
                            contest,
                            ContestCancel::Pointer(CancelReason::Platform),
                        );
                    }
                });
                keep_first(&mut first_panic, settled, "touchpad swipe arena end");
                let dropped = caught(|| drop(open));
                keep_first(&mut first_panic, dropped, "touchpad swipe path");
            }
        }
        first_panic.resume();
    }

    /// End a swipe the binding closed before its platform did: its path hears
    /// a `Cancelled`, and its contest is abandoned without a winner.
    pub(super) fn cancel_touchpad_swipe(&self, open: OpenSwipe, first_panic: &mut FirstPanic) {
        let mut cancel = open.last;
        cancel.phase = TouchpadSwipePhase::Cancelled;
        let contest = self.arena.detach(open.pointer());
        let delivered = self.dispatch_ephemeral(&PointerEvent::TouchpadSwipe(cancel), &open.path);
        keep_first(first_panic, delivered, "touchpad swipe cancel");
        let abandoned = caught(|| {
            GestureArena::abandon_detached(contest, ContestCancel::Abandoned);
        });
        keep_first(first_panic, abandoned, "touchpad swipe arena abandon");
        let dropped = caught(|| drop(open));
        keep_first(first_panic, dropped, "touchpad swipe path");
    }
}
