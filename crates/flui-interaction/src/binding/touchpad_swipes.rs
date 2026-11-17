//! Open touchpad swipes: the hit path each one took at its start.
//!
//! A swipe is a gesture, not a scroll: its `Start` is hit-tested once where
//! the pointer rests, and every later phase is delivered along that path,
//! wherever the content under the pointer moves meanwhile. The arena keys its
//! contest by the swipe's pointer, so the recognizers its `Start` reached
//! compete for it as they would for a contact.

use std::cell::RefCell;

use flui_platform::pointer::{DeviceId, TouchpadSwipeEvent};
use smallvec::SmallVec;

use super::pointer_sequences::MAX_SIMULTANEOUS_POINTERS;
use crate::{
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
