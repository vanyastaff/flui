//! The IME cursor-area loop: after each frame it hands the platform the
//! caret's (or the composing region's) global rectangle, so the candidate
//! window follows the caret.

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::Bounds;
use flui_interaction::TextInputHandle;
use flui_platform::text_store::OwnerCalls;
use flui_rendering::pipeline::PipelineCell;

use super::bounds_from_rect;
use crate::text::text_store::{EditableTextStore, with_editable_global};

/// The self-rescheduling IME cursor-area tracking loop (ADR-0030).
///
/// One instance is created per IME attach (focus gain). Each firing reads the
/// caret's current global rect and forwards it through
/// [`TextInputHandle::set_cursor_area`] when it changed, then reschedules
/// itself for the next completed frame, dormant whenever no frame runs.
/// `Clone` because
/// [`flui_scheduler::PostFrameHandle::schedule`] takes an `FnOnce`, so the only way to
/// make it self-rescheduling without boxing a trait object is for each
/// firing to consume `self` and, if still alive, construct the next firing's
/// closure from a fresh clone of the same capture.
#[derive(Clone)]
pub(super) struct CursorAreaLoop {
    pub(super) post_frame: flui_scheduler::PostFrameHandle,
    pub(super) pipeline_owner: Option<PipelineCell>,
    /// The `EditableTextRenderView`'s own inner anchor (ADR-0030) — see
    /// `EditableTextState::inner_anchor`'s doc.
    pub(super) inner_anchor: flui_objects::SubtreeAnchor,
    pub(super) text_input: TextInputHandle,
    /// The field's text store, told when the rect moves (TSF's
    /// `OnLayoutChange`) and given a chance each frame to report app edits.
    /// Weak: the loop must not keep a disposed field's store alive.
    pub(super) store: std::rc::Weak<EditableTextStore>,
    /// Per-attach liveness flag — see `EditableTextState::cursor_area_alive`'s
    /// doc for why it is fresh per attach. Checked at the START of every
    /// firing so a callback already queued when the attach ended dies
    /// silently instead of resurrecting a stale loop or running alongside a
    /// newer one.
    pub(super) alive: Rc<Cell<bool>>,
    /// The last rect actually sent. Fresh per attach (never shared across
    /// attaches, see `EditableTextState::init_state`'s IME focus listener) —
    /// a brand-new IME session must always get the first rect, even at an
    /// unchanged caret position.
    pub(super) last_sent: Rc<Cell<Option<Bounds<f64>>>>,
}

impl CursorAreaLoop {
    /// Register the next firing. Every `schedule` failure is warned,
    /// never silent: a loop that stops rescheduling without a diagnostic is
    /// a candidate window stuck at (0, 0) with no signal anything is wrong.
    pub(super) fn schedule(self) {
        let post_frame = self.post_frame.clone();
        if let Err(error) = post_frame.schedule(move |_timing| self.fire()) {
            tracing::warn!(
                ?error,
                "IME cursor-area tick could not be (re)scheduled; the platform \
                 candidate window will stop following the caret"
            );
        }
    }

    fn fire(self) {
        if !self.alive.get() {
            return;
        }
        // The observer, the platform and diagnostics are other code: each
        // runs though an earlier one failed, the loop is rescheduled, and the
        // first failure is resumed after that.
        let mut calls = OwnerCalls::new();
        let store = self.store.upgrade();
        if let Some(store) = &store {
            calls.run(|| store.controller_changed());
        }
        // A `None` read is a transient miss (the anchored subtree unmounted
        // mid-rebuild, or a transform is momentarily unavailable) — skip
        // this firing's send but keep the loop alive. Only `alive == false`
        // ever stops rescheduling.
        if let Some(rect) = self.global_caret_rect()
            && Some(rect) != self.last_sent.get()
        {
            match calls.run(|| self.text_input.set_cursor_area(rect)) {
                Some(Err(error)) => {
                    self.alive.set(false);
                    calls.run(|| {
                        tracing::warn!(
                            ?error,
                            "IME cursor-area tracking stopped because its presentation is unavailable"
                        );
                    });
                    calls.resume();
                    return;
                }
                Some(Ok(())) => {
                    self.last_sent.set(Some(rect));
                    if let Some(store) = &store {
                        calls.run(|| store.layout_changed());
                    }
                }
                None => {}
            }
        }
        // A read the store answered `NoLayout` inside an edit session owes the
        // input method a notification once layout exists, even when the rect
        // is the one already sent.
        if let Some(store) = &store {
            calls.run(|| store.settle_owed_layout());
        }
        calls.run(|| self.schedule());
        calls.resume();
    }

    /// The IME candidate window's current target rect in window-root-space
    /// logical pixels: the anchored `RenderEditable`'s committed transform
    /// to the render root (the text store's geometry uses the same one;
    /// it equals `inner_anchor`'s, since `RenderSubtreeAnchor` lays its
    /// child out at its own origin), applied to the editable's composing
    /// region rect when one is active, falling back to its collapsed caret
    /// rect otherwise (prefer the composing rect, fall back to the caret rect
    /// when none is available). ADR-0030
    /// upgrades this loop from the caret-rect-only reduction ADR-0030
    /// originally landed.
    ///
    /// `None` also when the pipeline is checked out by a frame phase
    /// (`with_editable_global` uses `try_with`): the loop fires as a
    /// post-frame callback, after the pipeline is released, so this only
    /// skips one firing's send instead of panicking, and the next firing
    /// retries.
    fn global_caret_rect(&self) -> Option<Bounds<f64>> {
        let owner = self.pipeline_owner.as_ref()?;
        with_editable_global(owner, &self.inner_anchor, |editable, to_root| {
            let local_rect = editable
                .rect_for_composing_range()
                .unwrap_or_else(|| editable.caret_local_rect());
            Some(bounds_from_rect(to_root.transform_rect(&local_rect)))
        })
    }
}
