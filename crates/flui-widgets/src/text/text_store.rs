//! [`EditableTextStore`]: a mounted [`EditableText`](super::EditableText)
//! as the text store an input method pulls from (ADR-0090).
//!
//! The store is built over the field's existing state — the
//! [`TextEditingController`] and the laid-out `RenderEditable` — until the
//! per-realm editing state of ADR-0092 replaces them behind the same trait.
//! The controller's UTF-8 offsets are converted at this boundary with
//! `flui_platform_api::text_store::utf16`, and nowhere else.
//!
//! # Sessions
//!
//! A read session answers from a snapshot of the controller taken when the
//! lock opens. A read-write session edits a working copy of that snapshot
//! and writes it back to the controller once, when the session ends: the
//! field sees a platform session as one change, whatever it did inside.
//! The write is dropped if the application changed the controller since the
//! snapshot (its generation moved), so an application edit is never
//! overwritten by a session that did not see it.
//!
//! # The owner
//!
//! The session's one listener notification and at most one `on_changed`
//! call are owed, not made, inside the grant: they run in `settle`, which the
//! arbiter calls once the lock is released and before the next grant, so
//! owner code may request a lock or edit the field (ADR-0090 amendment).
//! `on_changed` runs only when the committed text — the text without the
//! composition — changed, and receives it.
//!
//! # What reaches the observer
//!
//! Only changes the platform did not make: an edit through the controller
//! (typing, a paste, `set_text`, a swapped controller) is diffed against the
//! text and selection the store last reported and sent as one
//! `TextChange`, and a moved caret rect as `layout_changed`. A key edit is
//! reported at once; any other app edit at the next frame or lock request
//! (see [`EditableTextStore::controller_changed`]). A platform session's own
//! write-back updates that record, so it is never echoed. No notification is
//! sent while a lock is held: a change made then is reported when the lock
//! is released.
//!
//! # Geometry
//!
//! Rects and points are in window-root logical pixels, mapped through the
//! editable's committed transform. A query made while the laid-out text is
//! not the text the session sees (an edit earlier in the same session, or an
//! app edit not yet laid out) is [`TextStoreError::NoLayout`], TSF's
//! `TS_E_NOLAYOUT`. An obscured field answers through the mask: one mask
//! character per source grapheme cluster.

use std::cell::{Cell, RefCell};
use std::ops::Range;

use std::rc::Rc;

use flui_foundation::geometry::{Bounds, Point};
use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_interaction::TextInputHandle;
use flui_objects::{RenderEditable, SubtreeAnchor};
use flui_painting::text_boundaries::graphemes;
use flui_platform_api::text_store::{
    CommitGate, Composition, CompositionLedger, LockArbiter, LockGrant, LockOutcome, LockTiming,
    OwnerCalls, PointMode, RangeRect, Selection, TextChange, TextStore, TextStoreEdit,
    TextStoreError, TextStoreObserver, TextStoreRead, TextStoreStatus, Utf16Offset, Utf16Range,
    committed_text, utf16,
};
use flui_rendering::pipeline::PipelineCell;

use super::controller::{self, ComposingState, TextEditingController};
use super::editable_text::{EditObserver, bounds_from_rect, obscure};

/// The committed `RenderEditable` under `inner_anchor` and its transform to
/// the render root, handed to `f`; `None` when the field is not mounted or
/// laid out, or the pipeline is busy.
///
/// `try_with`, not `with`: a platform query or a pointer event can arrive
/// while a frame phase holds the pipeline, and "the tree is busy" is a real
/// answer there, not a panic.
pub(super) fn with_editable_global<R>(
    owner: &PipelineCell,
    inner_anchor: &SubtreeAnchor,
    f: impl FnOnce(&RenderEditable, &Matrix4) -> Option<R>,
) -> Option<R> {
    let anchor_id = inner_anchor.get()?;
    owner
        .try_with(|owner| {
            let root_id = owner.root_id()?;
            let tree = owner.render_tree();
            let editable_id = *tree.children(anchor_id).first()?;
            let editable = tree
                .get(editable_id)?
                .as_box()?
                .render_object()
                .downcast_ref::<RenderEditable>()?; // the field reaches the one concrete render object type it mounts under `inner_anchor`, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure.
            let to_root = owner.transform_to(editable_id, root_id)?;
            f(editable, &to_root)
        })
        .flatten()
}

/// The field's document as one session sees it: the controller's fields,
/// in its own UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Doc {
    text: String,
    anchor: usize,
    caret: usize,
    composing: Option<(Range<usize>, bool)>,
    /// What the composition stands for in the committed text; empty
    /// without one.
    origin: String,
}

impl Doc {
    /// The text with the composing range replaced by its origin.
    fn committed(&self) -> String {
        committed_text(
            &self.text,
            self.composing
                .as_ref()
                .map(|(range, _)| (range.clone(), self.origin.as_str())),
        )
    }

    /// A ledger for a session opening on this document.
    fn ledger(&self) -> CompositionLedger {
        CompositionLedger::open(
            self.composing
                .as_ref()
                .map(|(range, _)| (range.clone(), self.origin.clone())),
        )
    }

    fn units(&self, byte: usize) -> Utf16Offset {
        utf16::utf16_offset(&self.text, byte).unwrap_or_else(|_| utf16::utf16_len(&self.text))
    }

    fn byte(&self, offset: Utf16Offset) -> Result<usize, TextStoreError> {
        Ok(utf16::byte_offset(&self.text, offset)?)
    }

    fn selection(&self) -> Selection {
        Selection {
            anchor: self.units(self.anchor),
            active: self.units(self.caret),
        }
    }

    fn composition(&self) -> Option<Composition> {
        self.composing
            .as_ref()
            .map(|(range, hides_caret)| Composition {
                range: Utf16Range::new(self.units(range.start), self.units(range.end))
                    .expect("BUG: a composing range is ordered"),
                hides_caret: *hides_caret,
            })
    }

    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        let bytes = utf16::byte_range(&self.text, range)?;
        self.text.replace_range(bytes.clone(), text);
        let end = bytes.start + text.len();
        self.anchor = end;
        self.caret = end;
        self.composing = self.composing.take().and_then(|(composed, hides_caret)| {
            if bytes.end <= composed.start {
                let shift = |offset: usize| offset + text.len() - bytes.len();
                Some((shift(composed.start)..shift(composed.end), hides_caret))
            } else if bytes.start >= composed.end {
                Some((composed, hides_caret))
            } else {
                None
            }
        });
        Ok(TextChange {
            start: range.start(),
            old_end: range.end(),
            new_end: Utf16Offset::new(range.start().get() + utf16::utf16_len(text).get()),
        })
    }
}

/// A mounted field's text store. Built in `EditableTextState::init_state`
/// and attached through the presentation's `TextInputHandle` while the field
/// is focused.
pub(super) struct EditableTextStore {
    controller: Rc<RefCell<TextEditingController>>,
    arbiter: LockArbiter,
    observer: RefCell<Option<Rc<dyn TextStoreObserver>>>,
    /// The text and selection last reported to the observer (or written by a
    /// platform session), which app-side edits are diffed against.
    reported: RefCell<(String, Selection)>,
    /// `false` once the field is disposed: every lock is then refused.
    alive: Cell<bool>,
    /// A layout or status change the observer has not heard of yet, because
    /// it happened while the store could not notify.
    layout_dirty: Cell<bool>,
    status_dirty: Cell<bool>,
    /// What sessions that wrote the controller back owe its listeners and
    /// `on_changed`, in commit order: delivered in [`Self::settle`], once
    /// the lock is released.
    owed: RefCell<Vec<Owed>>,
    /// The presentation's text input, whose closing detaches the store.
    /// Whether commits are allowed is the gate it installs on attach, which
    /// the arbiter reads; with no presentation IME the arbiter's own gate
    /// stays open.
    handle: Option<TextInputHandle>,
    pipeline: Option<PipelineCell>,
    inner_anchor: SubtreeAnchor,
    obscure: Rc<Cell<bool>>,
    obscuring_character: Rc<Cell<char>>,
    edits: EditObserver,
}

/// The pieces of the mounted field a store reads.
pub(super) struct FieldParts {
    pub(super) controller: Rc<RefCell<TextEditingController>>,
    pub(super) handle: Option<TextInputHandle>,
    pub(super) pipeline: Option<PipelineCell>,
    pub(super) inner_anchor: SubtreeAnchor,
    pub(super) obscure: Rc<Cell<bool>>,
    pub(super) obscuring_character: Rc<Cell<char>>,
    pub(super) edits: EditObserver,
}

impl EditableTextStore {
    pub(super) fn new(parts: FieldParts) -> Rc<Self> {
        let store = Self {
            controller: parts.controller,
            arbiter: LockArbiter::new(),
            observer: RefCell::new(None),
            reported: RefCell::new((String::new(), Selection::collapsed(Utf16Offset::ZERO))),
            alive: Cell::new(true),
            layout_dirty: Cell::new(false),
            status_dirty: Cell::new(false),
            owed: RefCell::new(Vec::new()),
            handle: parts.handle,
            pipeline: parts.pipeline,
            inner_anchor: parts.inner_anchor,
            obscure: parts.obscure,
            obscuring_character: parts.obscuring_character,
            edits: parts.edits,
        };
        let doc = store.read_doc();
        *store.reported.borrow_mut() = (doc.text.clone(), doc.selection());
        Rc::new(store)
    }

    /// The controller may have changed. Report whatever the platform did not
    /// do itself, unless a lock is held — then it is reported on release.
    ///
    /// Called where the field can reach the store on the owner thread: after
    /// a key edit, on a controller swap, before and after every lock, and
    /// once per frame from the cursor-area loop. Not from a controller
    /// listener: those are `Send + Sync` and this store is not, so an edit
    /// made from elsewhere (`set_text`, a paste) is reported at the next of
    /// those points.
    pub(super) fn controller_changed(&self) {
        if self.may_notify() {
            let mut calls = OwnerCalls::new();
            self.report_app_changes(&mut calls);
            calls.resume();
        }
    }

    /// The field's caret or composition rect moved. Reported now if the
    /// store may notify, otherwise at the next point it may.
    pub(super) fn layout_changed(&self) {
        self.layout_dirty.set(true);
        self.flush_notifications();
    }

    /// The field's obscuring changed, so `status().protected` did.
    pub(super) fn status_changed(&self) {
        self.status_dirty.set(true);
        self.flush_notifications();
    }

    /// Whether the observer may hear from the store now: no lock is held and
    /// commits are allowed, so a sink that answers a notification with a
    /// synchronous lock request (TSF's do) is granted it.
    fn may_notify(&self) -> bool {
        !self.arbiter.is_locked() && self.may_commit().unwrap_or(false)
    }

    /// Send every notification that waited for [`Self::may_notify`].
    fn flush_notifications(&self) {
        if !self.may_notify() {
            return;
        }
        // Every notification owed is sent though an earlier one panicked; the
        // first panic is resumed once all were.
        let mut calls = OwnerCalls::new();
        self.report_app_changes(&mut calls);
        if self.status_dirty.replace(false) {
            self.notify(&mut calls, |observer| observer.status_changed());
        }
        if self.layout_dirty.replace(false) {
            self.notify(&mut calls, |observer| observer.layout_changed());
        }
        calls.resume();
    }

    /// The field is gone: refuse every later lock and drop queued ones
    /// unrun, so a deferred edit never lands in a buffer that is no longer
    /// this field's.
    pub(super) fn detach(&self) {
        self.alive.set(false);
        let observer = self.observer.borrow_mut().take();
        let mut calls = OwnerCalls::new();
        calls.run(|| self.arbiter.clear());
        calls.retire(observer);
        calls.resume();
    }

    /// Run queued grants before a key edit, so a key typed after an IME
    /// commit that is still queued lands after it. Inside a frame
    /// transaction the grants cannot run and the key goes first.
    pub(super) fn run_deferred_before_app_edit(&self) {
        let _ran = self.run_deferred_grants();
    }

    /// Whether the gate the presentation installed on attach is open; an
    /// error once the field or its presentation is gone.
    fn may_commit(&self) -> Result<bool, TextStoreError> {
        if !self.alive.get() {
            return Err(TextStoreError::Detached);
        }
        if let Some(handle) = &self.handle {
            handle.ensure_open().map_err(|_| TextStoreError::Detached)?;
        }
        Ok(self.arbiter.may_commit())
    }

    /// Tell the observer, inside `calls`.
    fn notify(&self, calls: &mut OwnerCalls, call: impl FnOnce(&dyn TextStoreObserver)) {
        let observer = self.observer.borrow().clone();
        if let Some(observer) = observer {
            calls.run(|| call(&*observer));
            // An observer that replaced or cleared itself left this clone
            // its last owner.
            calls.retire(observer);
        }
    }

    fn read_doc(&self) -> Doc {
        self.snapshot().1
    }

    /// The controller, its document and its generation, read in one
    /// critical section.
    fn snapshot(&self) -> (TextEditingController, Doc, u64) {
        let controller = self.controller.borrow().clone();
        let (doc, generation) = controller.with_inner(|inner| {
            let doc = Doc {
                text: inner.text.clone(),
                anchor: inner.selection.anchor,
                caret: inner.selection.caret,
                composing: inner
                    .composing
                    .as_ref()
                    .map(|state| (state.range.clone(), state.caret_hidden)),
                origin: inner
                    .composing
                    .as_ref()
                    .map(|state| state.origin.clone())
                    .unwrap_or_default(),
            };
            (doc, inner.generation)
        });
        (controller, doc, generation)
    }

    /// Diff the controller against what was last reported and tell the
    /// observer.
    fn report_app_changes(&self, calls: &mut OwnerCalls) {
        let doc = self.read_doc();
        let selection = doc.selection();
        let (change, moved) = {
            let mut reported = self.reported.borrow_mut();
            let change = (reported.0 != doc.text).then(|| text_change(&reported.0, &doc.text));
            let moved = change.is_some() || reported.1 != selection;
            *reported = (doc.text, selection);
            (change, moved)
        };
        if let Some(change) = change {
            self.notify(calls, |observer| observer.text_changed(change));
        }
        if moved {
            self.notify(calls, |observer| observer.selection_changed());
        }
    }

    fn open(&self, grant: LockGrant) {
        match grant {
            LockGrant::Read(body) => {
                let doc = self.read_doc();
                let ledger = doc.ledger();
                body(&Session {
                    store: self,
                    doc,
                    ledger,
                });
            }
            LockGrant::ReadWrite(body) => {
                let (controller, original, generation) = self.snapshot();
                let mut session = Session {
                    store: self,
                    doc: original.clone(),
                    ledger: original.ledger(),
                };
                body(&mut session);
                session
                    .ledger
                    .origin()
                    .unwrap_or_default()
                    .clone_into(&mut session.doc.origin);
                if session.doc != original {
                    self.write_back(&controller, generation, session.doc, &original);
                }
            }
        }
    }

    /// Apply a platform session's result to the controller in one write,
    /// unless the application changed the field since the session opened at
    /// `generation`: then its edit stays, the session is dropped, and the
    /// platform hears of the edit once the lock is released (ADR-0090
    /// amendment item 3).
    ///
    /// The listeners and `on_changed` are owed, not called: they run in
    /// [`Self::settle`], after the lock is released.
    fn write_back(
        &self,
        controller: &TextEditingController,
        generation: u64,
        doc: Doc,
        original: &Doc,
    ) {
        let current = self.controller.borrow().clone();
        if !current.is_same_controller(controller) {
            return;
        }
        let reported = (doc.text.clone(), doc.selection());
        let committed_after = doc.committed();
        let applied = controller.with_inner_silent(|inner| {
            if inner.generation != generation {
                return false;
            }
            inner.selection = controller::Selection {
                anchor: doc.anchor,
                caret: doc.caret,
            };
            let origin = doc.origin;
            inner.composing = doc.composing.map(|(range, caret_hidden)| ComposingState {
                range,
                caret_hidden,
                origin,
            });
            inner.text = doc.text;
            true
        });
        if !applied {
            return;
        }
        *self.reported.borrow_mut() = reported;
        let committed = (committed_after != original.committed()).then_some(committed_after);
        self.owed.borrow_mut().push(Owed {
            controller: controller.clone(),
            committed,
        });
    }

    /// Deliver what a finished grant owes, now that its lock is released and
    /// before the next grant runs: for each session written back, in commit
    /// order, `on_changed` with the committed text that session produced
    /// (when it changed), then the controller's listeners; then the
    /// observer's notifications.
    ///
    /// Every obligation and its value is taken before any owner code runs,
    /// so a session that code opens owes, and settles, its own, after the
    /// owner heard of this one. The observer is told even when owner code
    /// panics, so a grant queued behind this one never runs before the
    /// platform hears of an edit the owner made; the first panic is then
    /// resumed for the arbiter to park.
    fn settle(&self) {
        let owed = std::mem::take(&mut *self.owed.borrow_mut());
        let mut calls = OwnerCalls::new();
        for Owed {
            controller,
            committed,
        } in owed
        {
            if let Some(committed) = committed
                && self.alive.get()
            {
                self.edits.deliver(&committed, &mut calls);
            }
            calls.run(|| controller.notify_changed());
            calls.retire(controller);
        }
        calls.run(|| self.flush_notifications());
        calls.resume();
    }

    /// The text the render object shows for `source`.
    fn rendered(&self, source: &str) -> String {
        if self.obscure.get() {
            obscure(source, &mut [], self.obscuring_character.get())
        } else {
            source.to_owned()
        }
    }

    /// The rendered byte range for a source byte range: the same range, or
    /// on an obscured field the mask characters of the clusters it touches.
    fn rendered_range(&self, source: &str, bytes: Range<usize>) -> Range<usize> {
        if !self.obscure.get() {
            return bytes;
        }
        let width = self.obscuring_character.get().len_utf8();
        let (mut first, mut last) = (0, 0);
        for (index, cluster) in graphemes(source).enumerate() {
            if cluster.end <= bytes.start {
                first = index + 1;
            }
            if cluster.start < bytes.end {
                last = index + 1;
            }
        }
        if bytes.is_empty() {
            last = first;
        }
        first * width..last * width
    }

    /// The source byte offset for a rendered one.
    fn source_offset(&self, source: &str, rendered: usize) -> usize {
        if self.obscure.get() {
            super::editable_text::source_offset_for_masked_offset(
                source,
                rendered,
                self.obscuring_character.get(),
            )
        } else {
            rendered
        }
    }

    /// Run `f` against the laid-out editable when it shows `doc`'s text.
    fn with_layout<R>(
        &self,
        doc: &Doc,
        f: impl FnOnce(&RenderEditable, &Matrix4) -> Result<R, TextStoreError>,
    ) -> Result<R, TextStoreError> {
        let pipeline = self.pipeline.as_ref().ok_or(TextStoreError::NoLayout)?;
        let rendered = self.rendered(&doc.text);
        with_editable_global(pipeline, &self.inner_anchor, |editable, to_root| {
            (editable.plain_text() == rendered).then(|| f(editable, to_root))
        })
        .unwrap_or(Err(TextStoreError::NoLayout))
    }

    /// The allocated field box, independently of the shaped line's extent.
    fn viewport(&self) -> Result<Rect, TextStoreError> {
        let pipeline = self.pipeline.as_ref().ok_or(TextStoreError::NoLayout)?;
        let anchor = self.inner_anchor.get().ok_or(TextStoreError::NoLayout)?;
        pipeline
            .try_with(|owner| {
                let tree = owner.render_tree();
                let editable = *tree.children(anchor).first()?;
                let size = tree.get(editable)?.size()?;
                Some(Rect::from_origin_size(Point::ZERO, size))
            })
            .flatten()
            .ok_or(TextStoreError::NoLayout)
    }
}

/// What one session written back owes, fixed when it was written: the
/// controller whose listeners hear of it, and the committed text it produced
/// when that changed, which `on_changed` receives.
struct Owed {
    controller: TextEditingController,
    committed: Option<String>,
}

/// The smallest change turning `old` into `new`, as `TS_TEXTCHANGE` in
/// UTF-16 units: the common prefix and suffix (whole scalars) are kept.
fn text_change(old: &str, new: &str) -> TextChange {
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let suffix: usize = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let units = |text: &str, byte: usize| {
        utf16::utf16_offset(text, byte).expect("BUG: a whole-scalar prefix ends on a boundary")
    };
    TextChange {
        start: units(old, prefix),
        old_end: units(old, old.len() - suffix),
        new_end: units(new, new.len() - suffix),
    }
}

impl TextStore for EditableTextStore {
    fn status(&self) -> TextStoreStatus {
        TextStoreStatus::EDITABLE_SINGLE_LINE.with_protected(self.obscure.get())
    }

    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        self.may_commit()?;
        // An app edit not yet reported is reported before the platform's
        // session can see (and write back over) it.
        self.flush_notifications();
        let outcome =
            self.arbiter
                .request(grant, timing, &mut |grant| self.open(grant), &mut || {
                    self.settle();
                });
        self.flush_notifications();
        outcome
    }

    /// Also the store's commit anchor for notifications: whatever waited
    /// for the frame transaction to end is reported here, before and after
    /// the queued grants run.
    fn run_deferred_grants(&self) -> usize {
        if self.may_commit().is_err() {
            return 0;
        }
        self.flush_notifications();
        let ran = self
            .arbiter
            .run_deferred(&mut |grant| self.open(grant), &mut || self.settle());
        self.flush_notifications();
        ran
    }

    fn set_commit_gate(&self, gate: CommitGate) {
        self.arbiter.set_gate(gate);
    }

    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>) {
        // The replaced observer retires after the borrow is released.
        let previous = std::mem::replace(&mut *self.observer.borrow_mut(), observer);
        let mut calls = OwnerCalls::new();
        calls.retire(previous);
        calls.resume();
    }
}

/// One lock's view of the field: the snapshot (read) or working copy
/// (read-write) of its document.
struct Session<'a> {
    store: &'a EditableTextStore,
    doc: Doc,
    /// What the composition stands for as the session edits it.
    ledger: CompositionLedger,
}

impl Session<'_> {
    fn local_rect(
        &self,
        editable: &RenderEditable,
        range: Utf16Range,
    ) -> Result<Rect, TextStoreError> {
        let bytes = utf16::byte_range(&self.doc.text, range)?;
        let rendered = self.store.rendered_range(&self.doc.text, bytes);
        local_rect(editable, rendered).ok_or(TextStoreError::NoLayout)
    }
}

/// The local rect of a rendered byte range: the caret at its start when it
/// is empty; otherwise the span between the carets at its two ends, as tall
/// as the range's boxes.
///
/// The horizontal extent comes from caret positions, not the glyph boxes, so
/// a rect query and a point query ([`rendered_offset_at`]) agree even inside
/// a cluster the shaper drew as several glyphs sharing one byte range, where
/// the boxes of a sub-range span every glyph of the cluster.
fn local_rect(editable: &RenderEditable, rendered: Range<usize>) -> Option<Rect> {
    let start = editable.local_rect_for_range(rendered.start..rendered.start)?;
    if rendered.is_empty() {
        return Some(start);
    }
    let end = editable.local_rect_for_range(rendered.end..rendered.end)?;
    let boxes = editable.local_rect_for_range(rendered).unwrap_or(start);
    let (left, right) = if start.left() <= end.left() {
        (start.left(), end.left())
    } else {
        (end.left(), start.left())
    };
    Some(Rect::from_ltrb(
        left,
        boxes.top().min(start.top()),
        right,
        boxes.bottom().max(start.bottom()),
    ))
}

impl TextStoreRead for Session<'_> {
    fn document_len(&self) -> Utf16Offset {
        utf16::utf16_len(&self.doc.text)
    }

    fn text(&self, range: Utf16Range) -> Result<String, TextStoreError> {
        let bytes = utf16::byte_range(&self.doc.text, range)?;
        if self.store.obscure.get() {
            return Err(TextStoreError::Protected);
        }
        Ok(self.doc.text[bytes].to_owned())
    }

    fn selection(&self) -> Selection {
        self.doc.selection()
    }

    fn composition(&self) -> Option<Composition> {
        self.doc.composition()
    }

    fn rect_for_range(&self, range: Utf16Range) -> Result<RangeRect, TextStoreError> {
        utf16::byte_range(&self.doc.text, range)?;
        let viewport = self.store.viewport()?;
        self.store.with_layout(&self.doc, |editable, to_root| {
            let local = self.local_rect(editable, range)?;
            Ok(RangeRect {
                bounds: bounds_from_rect(to_root.transform_rect(&local)),
                clipped: local.left() < viewport.left()
                    || local.top() < viewport.top()
                    || local.right() > viewport.right()
                    || local.bottom() > viewport.bottom(),
            })
        })
    }

    fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
        self.store.with_layout(&self.doc, |editable, to_root| {
            let len = editable.plain_text().len();
            let text = editable
                .local_rect_for_range(0..len)
                .ok_or(TextStoreError::NoLayout)?;
            let caret = editable
                .local_rect_for_range(len..len)
                .ok_or(TextStoreError::NoLayout)?;
            Ok(bounds_from_rect(
                to_root.transform_rect(&text.union(&caret)),
            ))
        })
    }

    fn index_at_point(
        &self,
        point: Point<f64>,
        mode: PointMode,
    ) -> Result<Utf16Offset, TextStoreError> {
        let viewport = self.store.viewport()?;
        let rendered = self.store.with_layout(&self.doc, |editable, to_root| {
            let (x, y) = to_root
                .try_inverse()
                .ok_or(TextStoreError::NoLayout)?
                .transform_point(point.x, point.y);
            if mode == PointMode::Exact && !viewport.contains(Point::new(x, y)) {
                return Err(TextStoreError::PointOutside);
            }
            rendered_offset_at(editable, Offset::new(x, y), mode)
        })?;
        let source = self.store.source_offset(&self.doc.text, rendered);
        Ok(self.doc.units(source))
    }
}

/// The rendered byte offset at a local point: the scalar whose caret-to-caret
/// span contains it (`Exact`), or the nearest scalar boundary (`Nearest`).
/// Always a scalar boundary of the rendered text.
fn rendered_offset_at(
    editable: &RenderEditable,
    point: Offset<f64>,
    mode: PointMode,
) -> Result<usize, TextStoreError> {
    let text = editable.plain_text();
    let boundaries: Vec<(usize, f64)> = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain([text.len()])
        .map(|byte| {
            editable
                .local_rect_for_range(byte..byte)
                .map(|caret| (byte, caret.left()))
                .ok_or(TextStoreError::NoLayout)
        })
        .collect::<Result<_, _>>()?;
    // The line box: the text's, or the caret's when there is no text.
    let line = editable
        .local_rect_for_range(0..text.len())
        .or_else(|| editable.local_rect_for_range(0..0))
        .ok_or(TextStoreError::NoLayout)?;
    let (x, y) = (point.dx, point.dy);
    match mode {
        PointMode::Exact => {
            let inside_line = line.top() <= y && y < line.bottom();
            boundaries
                .windows(2)
                .find(|pair| pair[0].1.min(pair[1].1) <= x && x < pair[0].1.max(pair[1].1))
                .filter(|_| inside_line)
                .map(|pair| pair[0].0)
                .ok_or(TextStoreError::PointOutside)
        }
        PointMode::Nearest => Ok(boundaries
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |&(byte, _)| byte)),
    }
}

impl TextStoreEdit for Session<'_> {
    fn replace(&mut self, range: Utf16Range, text: &str) -> Result<TextChange, TextStoreError> {
        let bytes = utf16::byte_range(&self.doc.text, range)?;
        self.ledger.replace(&self.doc.text, bytes, text.len());
        self.doc.replace(range, text)
    }

    fn insert_at_selection(&mut self, text: &str) -> Result<TextChange, TextStoreError> {
        let range = self.doc.selection().range();
        self.replace(range, text)
    }

    fn set_selection(&mut self, selection: Selection) -> Result<(), TextStoreError> {
        let anchor = self.doc.byte(selection.anchor)?;
        let caret = self.doc.byte(selection.active)?;
        self.doc.anchor = anchor;
        self.doc.caret = caret;
        Ok(())
    }

    fn set_composition(&mut self, composition: Option<Composition>) -> Result<(), TextStoreError> {
        self.doc.composing = match composition {
            Some(composition) => {
                let range = utf16::byte_range(&self.doc.text, composition.range)?;
                Some((range, composition.hides_caret))
            }
            None => None,
        };
        let range = self.doc.composing.as_ref().map(|(range, _)| range.clone());
        self.ledger.set_composition(&self.doc.text, range);
        Ok(())
    }
}
