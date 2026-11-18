//! The pointer frame: the contacts a platform reported as one instant.
//!
//! A touch screen reports every finger it sampled at once, and a window
//! reports the motion of one display frame together. A recognizer that
//! measures the relation between contacts (a pinch) must see the state after
//! all of them moved, not the states in between, where one finger has moved
//! and the other has not. While a frame is open a recognizer records what each
//! event told it and joins the frame; when the outermost frame ends, every
//! joined recognizer settles once.

use crate::containment::keep_first;
use std::{
    cell::{Cell, RefCell},
    rc::Weak,
};

use super::GestureArena;
use crate::{
    containment::{FirstPanic, caught},
    gesture::recognizers::GestureRecognizer,
};

/// The frames open on one arena, and the recognizers waiting on them.
#[derive(Default)]
pub(super) struct PointerFrames {
    depth: Cell<usize>,
    joined: RefCell<Vec<Weak<dyn GestureRecognizer>>>,
}

impl GestureArena {
    /// Whether a pointer frame is open: the recognizer may then defer what
    /// depends on all of the frame's contacts to
    /// [`GestureRecognizer::pointer_frame_end`] by joining it.
    #[must_use]
    pub fn in_pointer_frame(&self) -> bool {
        self.shared.frames.depth.get() > 0
    }

    /// Ask for [`GestureRecognizer::pointer_frame_end`] when the open frame
    /// ends, once however many events of the frame joined. Returns `false`
    /// when no frame is open: nothing will call the hook, and the recognizer
    /// settles now.
    pub fn join_pointer_frame(&self, member: Weak<dyn GestureRecognizer>) -> bool {
        let frames = &self.shared.frames;
        if frames.depth.get() == 0 {
            return false;
        }
        let mut joined = frames.joined.borrow_mut();
        if !joined.iter().any(|known| known.ptr_eq(&member)) {
            joined.push(member);
        }
        true
    }

    /// Open a pointer frame. Frames nest: only the outermost one ends the
    /// frame for its members.
    pub(crate) fn begin_pointer_frame(&self) {
        let depth = &self.shared.frames.depth;
        depth.set(depth.get().saturating_add(1));
    }

    /// Close a pointer frame. The outermost one settles every recognizer that
    /// joined, in the order they joined, and returns the first failure after
    /// all of them ran. A recognizer that joins while they settle is settled
    /// in the same pass.
    pub(crate) fn end_pointer_frame(&self) -> FirstPanic {
        let frames = &self.shared.frames;
        if frames.depth.get() > 1 {
            frames.depth.set(frames.depth.get() - 1);
            return FirstPanic::new();
        }
        let mut first = FirstPanic::new();
        loop {
            let batch = std::mem::take(&mut *frames.joined.borrow_mut());
            if batch.is_empty() {
                break;
            }
            for member in batch {
                let Some(member) = member.upgrade() else {
                    continue;
                };
                let settled = caught(|| member.pointer_frame_end());
                keep_first(&mut first, settled, "pointer frame end");
            }
        }
        frames.depth.set(0);
        first
    }
}
