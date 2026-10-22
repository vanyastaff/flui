//! What the winit loop hands its host's drag and drop: winit reports files only.

use std::collections::HashMap;
use std::path::PathBuf;

use flui_foundation::geometry::Point;
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::{DragDropEvent, WindowId};

/// The host's drag-and-drop transport as the winit loop reaches it.
///
/// winit reports a file dragged over a window, dropped on it or the drag
/// cancelled, and nothing else; the loop hands each report to the transport,
/// which keeps the session and answers the event the window gets, if any. No
/// method calls user code or dispatches: the loop dispatches what they return,
/// with no lock held (ADR-0038 §5). The transport is the host's one
/// [`DataTransferSource`].
pub(crate) trait FileDrops: DataTransferSource {
    /// A `HoveredFile` over `window`, at `position` when the cursor is known.
    fn note_hovered_file(
        &self,
        window: WindowId,
        path: PathBuf,
        position: Option<Point<f64>>,
    ) -> Option<DragDropEvent>;

    /// A `DroppedFile` on `window`, at `position` when the cursor is known.
    fn note_dropped_file(
        &self,
        window: WindowId,
        path: PathBuf,
        position: Option<Point<f64>>,
    ) -> Option<DragDropEvent>;

    /// A `HoveredFileCancelled` for `window`.
    fn note_hover_cancelled(&self, window: WindowId) -> Option<DragDropEvent>;

    /// The windows whose drop is complete so far and waits for the end of the
    /// loop's turn; empty in the steady state.
    fn windows_awaiting_freeze(&self) -> Vec<WindowId>;

    /// The loop's turn ended: every waiting drop is complete, with the events
    /// to dispatch to each window at its last cursor `positions`.
    fn freeze_completed(
        &self,
        positions: &HashMap<WindowId, Point<f64>>,
    ) -> Vec<(WindowId, DragDropEvent)>;

    /// `window` is gone: what a drag over it held ends.
    fn forget_window(&self, window: WindowId);
}
