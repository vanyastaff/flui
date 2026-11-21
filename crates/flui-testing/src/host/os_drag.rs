//! The operating system's drag sessions as a headless window plays them.

#[cfg(doc)]
use super::HeadlessWindow;

use flui_platform::transfer::data::{RepresentationDescriptor, TransferActions};
use flui_platform::transfer::drag::{
    DragError, OsDragEnd, OsDragParts, OsDragRequest, OsDragStart, PlatformDragSource,
};
use flui_platform::{Unsupported, UnsupportedReason};

use parking_lot::Mutex;

/// The operating system's drag sessions as a [`HeadlessWindow`] plays them: a
/// modal session that ends inside its start, as Win32's `DoDragDrop` does,
/// the way a test chose with [`end_with`](Self::end_with). Until a test
/// chooses, the OS refuses every session and the drag stays in its window.
#[derive(Debug, Default)]
// `Mutex` fields: `PlatformDragSource` is `Send + Sync`.
pub struct HeadlessOsDrags {
    end: Mutex<Option<OsDragEnd>>,
    started: Mutex<Vec<StartedOsDrag>>,
}

/// What one escalated drag asked the OS for.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StartedOsDrag {
    /// What the session offers, ending with its identity.
    pub representations: Vec<RepresentationDescriptor>,
    /// The actions it advertises.
    pub allowed: TransferActions,
}

impl HeadlessOsDrags {
    /// End every later session with `end`, as a user dropping it on another
    /// application (or cancelling) would.
    pub fn end_with(&self, end: OsDragEnd) {
        *self.end.lock() = Some(end);
    }

    /// Every session started, in order.
    #[must_use]
    pub fn started(&self) -> Vec<StartedOsDrag> {
        self.started.lock().clone()
    }
}

impl PlatformDragSource for HeadlessOsDrags {
    fn start_timing(&self) -> OsDragStart {
        OsDragStart::OnLeavingWindow
    }

    fn start(&self, request: OsDragRequest) -> Result<(), DragError> {
        let end = (*self.end.lock()).ok_or(DragError::Unsupported(Unsupported::new(
            UnsupportedReason::NotRegistered,
        )))?;
        let OsDragParts { terms, on_end, .. } = request.into_parts();
        let allowed = terms.allowed();
        self.started.lock().push(StartedOsDrag {
            representations: terms.into_representations(),
            allowed,
        });
        on_end(end);
        Ok(())
    }
}
