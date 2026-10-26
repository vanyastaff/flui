//! The cursor a drag shows while it runs in its window (ADR-0211 §2).
//!
//! An in-window drag owns the window's cursor: hover is frozen while a moving
//! member owns the contact (ADR-0208 rule 6), so no region under the pointer
//! could say what a drop would do. The session shows its own state instead,
//! through an override of the presentation's mouse tracker, and gives the
//! cursor back when it ends or the OS takes the drag, whose cursor is the OS's.

use std::cell::RefCell;

use flui_platform::CursorIcon;
use flui_platform::data_transfer::TransferActions;
use flui_platform::pointer::PointerInfo;

use crate::events::PointerKind;
use crate::pointer::mouse_tracker::{CursorOverride, WeakMouseTracker};

/// What the window shows for a drag over nothing, over a receiver that
/// declines it, or over one that would take the drop with `accepted`.
///
/// A drop that would move shows the grabbing hand the drag lifted with, as a
/// drag over nothing does; one that would copy or link shows its badge; a
/// receiver that declines shows that the drop is refused there.
pub(crate) fn drag_cursor(over_receiver: bool, accepted: Option<TransferActions>) -> CursorIcon {
    match accepted {
        Some(action) if action.contains(TransferActions::COPY) => CursorIcon::Copy,
        Some(action) if action.contains(TransferActions::LINK) => CursorIcon::Alias,
        None if over_receiver => CursorIcon::NoDrop,
        Some(_) | None => CursorIcon::Grabbing,
    }
}

/// One session's hold on its presentation's cursor.
pub(crate) struct DragCursor {
    tracker: WeakMouseTracker,
    pointer: PointerInfo,
    shown: RefCell<Option<CursorOverride>>,
}

impl DragCursor {
    /// A hold for a drag by `pointer`; `None` for a pointer that has no cursor
    /// (a finger, a keyboard transfer).
    pub(crate) fn for_pointer(tracker: WeakMouseTracker, pointer: PointerInfo) -> Option<Self> {
        matches!(pointer.kind, PointerKind::Mouse | PointerKind::Pen { .. }).then(|| Self {
            tracker,
            pointer,
            shown: RefCell::new(None),
        })
    }

    /// Show `cursor` on the window. Publishing runs the presentation's cursor
    /// callback, so no borrow is held across it.
    pub(crate) fn show(&self, cursor: CursorIcon) {
        let current = self.shown.borrow_mut().take();
        let shown = match current {
            Some(shown) => {
                shown.set(cursor);
                Some(shown)
            }
            None => self
                .tracker
                .upgrade()
                .map(|tracker| tracker.override_cursor(self.pointer, cursor)),
        };
        let replaced = std::mem::replace(&mut *self.shown.borrow_mut(), shown);
        drop(replaced);
    }

    /// Give the window back to the hovered regions.
    pub(crate) fn hide(&self) {
        let shown = self.shown.borrow_mut().take();
        drop(shown);
    }
}
