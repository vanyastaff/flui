//! Whether the platform draws a field's selection UI while the field holds
//! the input ([`SelectionUi::Platform`], iOS's `UITextInteraction`): the field
//! then draws no caret, selection highlight, handles, magnifier or menu of its
//! own, and its taps, long presses and drags do not compete with the
//! platform's gestures inside it. It keeps drawing its text and its
//! composition's underline.
//!
//! The answer is the window's text-store host's, asked when the field
//! attaches its client (`TextInputHandle::selection_ui`), so it follows the
//! host that serves the field rather than the platform convention: a
//! convention names how a platform's own fields look and behave, and an iOS
//! convention under a host that draws nothing (a test, the Web on an iPhone)
//! still needs the field's own selection UI.

use std::cell::Cell;

use flui_foundation::RebuildReason;
use flui_platform::text_store::SelectionUi;
use flui_view::RebuildHandle;

/// Whether the platform draws the field's selection UI now, and the rebuild
/// that a change of it owes the field: the field's gestures and what it
/// draws are built from it.
pub(super) struct PlatformSelection {
    active: Cell<bool>,
    rebuild: RebuildHandle,
}

impl PlatformSelection {
    pub(super) fn new(rebuild: RebuildHandle) -> Self {
        Self {
            active: Cell::new(false),
            rebuild,
        }
    }

    /// Whether the platform draws the field's selection UI.
    pub(super) fn is_active(&self) -> bool {
        self.active.get()
    }

    /// The field attached its client to a host that answered `ui`, or holds
    /// the input no more (`None`). A change rebuilds the field; whether the
    /// platform took the selection UI over from it is returned, so the
    /// caller takes down what the field showed of its own.
    pub(super) fn follow(&self, ui: Option<SelectionUi>) -> bool {
        let active = ui == Some(SelectionUi::Platform);
        let was = self.active.replace(active);
        if was != active {
            self.rebuild.schedule(RebuildReason::StateChange);
        }
        active && !was
    }
}
