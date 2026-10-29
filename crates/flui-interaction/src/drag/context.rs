//! One presentation's way into its UI runtime's drag-and-drop: the lifecycle
//! capability the `Draggable` and `DropTarget` widgets acquire (ADR-0211).

use flui_foundation::PresentationId;
use flui_platform::PlatformWindow;
use flui_platform::data_transfer::TransferActions;
use flui_platform::drag::{AllowedActions, DragError};

use super::item::DragItem;
use super::owner::{DragHandle, DragSession, DragStart};
use super::receiver::DragPoint;
use crate::pointer::mouse_tracker::WeakMouseTracker;
use crate::pointer::{LocalPayloadTarget, MouseTracker};

/// Drag-and-drop as one presentation reaches it: drags that start in its
/// window, and the transfers its keyboard and assistive technology cut, copy
/// and paste (ADR-0211).
///
/// A widget acquires it in a lifecycle hook through
/// `LifecycleContext::drag_and_drop`, never in `build`. It holds its runtime's
/// sessions, window and cursor weakly: a widget keeping it keeps none of them
/// alive.
#[derive(Clone)]
pub struct DragAndDrop {
    handle: DragHandle,
    presentation: PresentationId,
    window: std::rc::Weak<dyn PlatformWindow>,
    cursor: WeakMouseTracker,
}

impl std::fmt::Debug for DragAndDrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragAndDrop")
            .field("presentation", &self.presentation)
            .finish_non_exhaustive()
    }
}

impl DragAndDrop {
    /// `presentation`'s drag-and-drop in the runtime `handle` names: its drags
    /// leave through `window` and show their state on `cursor`'s window.
    #[must_use]
    pub fn new(
        handle: DragHandle,
        presentation: PresentationId,
        window: std::rc::Weak<dyn PlatformWindow>,
        cursor: &MouseTracker,
    ) -> Self {
        Self {
            handle,
            presentation,
            window,
            cursor: cursor.downgrade(),
        }
    }

    /// A drag of `items` from this presentation's window, allowing `allowed`:
    /// it escalates through the window when the pointer leaves it, and a
    /// mouse or pen drag shows on the cursor whether a drop would move, copy,
    /// link or be refused where it is. Add the hit test, the contact and the
    /// observer, then [`start`](Self::start) it.
    pub fn drag(&self, items: Vec<DragItem>, allowed: AllowedActions) -> DragStart {
        let mut start =
            DragStart::new(self.presentation, items, allowed).window(self.window.clone());
        start.cursor = Some(self.cursor.clone());
        start
    }

    /// Start `start`.
    ///
    /// # Errors
    ///
    /// As [`DragHandle::start`].
    pub fn start(&self, start: DragStart) -> Result<DragSession, DragError> {
        self.handle.start(start)
    }

    /// Hold `start` for the next [`paste`](Self::paste): a cut when `action`
    /// is [`TransferActions::MOVE`], a copy when it is
    /// [`TransferActions::COPY`]. One transfer is held per runtime; holding
    /// another cancels the one held before. Escape, or dropping the session,
    /// cancels it too.
    ///
    /// # Errors
    ///
    /// [`DragError::NoAllowedActions`] when `action` is not a single action
    /// the drag allows within the app; otherwise as [`DragHandle::start`].
    pub fn hold(
        &self,
        start: DragStart,
        action: TransferActions,
    ) -> Result<DragSession, DragError> {
        self.handle.hold(start, action)
    }

    /// Paste the held transfer into the receiver registered under `target`,
    /// entered at `at`: it takes the drop as a drag released over it would,
    /// and the source hears the action it was dropped with. Returns that
    /// action; `None` when nothing is held, `target` names no receiver, or the
    /// receiver declined, in which case the transfer stays held.
    pub fn paste(&self, target: LocalPayloadTarget, at: DragPoint) -> Option<TransferActions> {
        self.handle.paste(self.presentation, target, at)
    }
}
