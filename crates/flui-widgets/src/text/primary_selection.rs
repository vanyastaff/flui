//! The primary selection a selectable text holds (X11 and Wayland): one rule
//! `EditableText` and `SelectionArea` share for when a selection is claimed
//! and let go.

use std::cell::{Cell, RefCell};

use flui_interaction::{
    PrimaryClaim, PrimarySelectionCapability, PrimarySelectionHandle, SelectionText,
};
use flui_platform::ClipboardRead;
use flui_platform::capability::Completion;
use flui_view::prelude::*;

/// A selectable text's hold on the platform's primary selection.
///
/// A non-empty selection holds a claim, which asks the text only when
/// another client pastes, so a drag that changes the selection on every
/// frame copies nothing. While the claim is current a changed selection
/// keeps it, since the text is read when asked; a claim another widget or
/// client took over is made again on the next change. An empty selection
/// lets go, and so does [`release`](Self::release).
///
/// Inert where the platform has no primary selection: the acquisition is
/// refused there, and nothing is claimed or read.
#[derive(Debug, Default)]
pub(crate) struct PrimarySelectionOwner {
    handle: Option<PrimarySelectionHandle>,
    claim: RefCell<Option<PrimaryClaim>>,
    /// Counts the reconciliations, so a claim that finds a newer one made
    /// while it ran yields to it; only compared for equality, so it may
    /// wrap.
    reconciled: Cell<u64>,
}

impl PrimarySelectionOwner {
    /// The presentation's primary selection, from a lifecycle hook.
    pub(crate) fn acquire(ctx: &dyn LifecycleContext) -> Self {
        Self {
            handle: ctx.capability::<PrimarySelectionCapability>().ok(),
            claim: RefCell::default(),
            reconciled: Cell::default(),
        }
    }

    /// Whether the platform has a primary selection to claim and read.
    pub(crate) fn is_available(&self) -> bool {
        self.handle.is_some()
    }

    /// The selection changed: `selected` says whether it is non-empty, and
    /// `text` makes what a claim asks for its text.
    ///
    /// No borrow is held while the backend runs, so a backend that reads
    /// the text at once, or a text that selects again, finds this owner
    /// usable.
    pub(crate) fn selection_changed<T: SelectionText>(
        &self,
        selected: bool,
        text: impl FnOnce() -> T,
    ) {
        let Some(handle) = &self.handle else {
            return;
        };
        if !selected {
            self.release();
            return;
        }
        let held = self.claim.take();
        if held.as_ref().is_some_and(PrimaryClaim::is_current) {
            // Kept: the text is read when asked, so it is the new one.
            let emptied = self.claim.replace(held);
            drop(emptied);
            return;
        }
        // A claim retires what it replaces, whose `Drop` may change the
        // selection and so reconcile again inside `claim`: that newer
        // reconciliation stands, and this claim yields to it.
        let reconciled = self.reconcile();
        // Claimed before the stale claim drops, so its drop finds another
        // current claim and releases nothing.
        let claim = handle.claim(text());
        if self.reconciled.get() != reconciled {
            // Stale beside a newer claim, so its drop releases nothing; still
            // current if the newer reconciliation let go, so it lets go too.
            drop((claim, held));
            return;
        }
        let replaced = self.claim.replace(Some(claim));
        drop((replaced, held));
    }

    /// Let go of the selection, if this text still holds it.
    pub(crate) fn release(&self) {
        self.reconcile();
        let released = self.claim.take();
        drop(released);
    }

    /// Count one more reconciliation of the claim with the selection, and
    /// return its number.
    fn reconcile(&self) -> u64 {
        let reconciled = self.reconciled.get().wrapping_add(1);
        self.reconciled.set(reconciled);
        reconciled
    }

    /// Read the primary selection's text; `None` where the platform has
    /// none.
    pub(crate) fn read_text(&self) -> Option<Completion<ClipboardRead>> {
        self.handle.as_ref().map(PrimarySelectionHandle::read_text)
    }
}
