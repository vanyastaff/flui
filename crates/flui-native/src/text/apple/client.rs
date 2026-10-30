//! The focused field's document as AppKit's `NSTextInputClient` reads and
//! edits it: the view's input-method verbs over the field's [`TextStore`]
//! (ADR-0090's AppKit mapping), and the window's [`TextStoreHost`].
//!
//! AppKit asks its client about the document it composes in (the selection,
//! a substring for reconversion, the index under a point, a range's
//! rectangle) and edits it with ranges (`replacementRange:`). A view with no
//! document of its own can answer neither, so the content view's methods come
//! here, and each one runs under the focused store's lock: a query under a
//! synchronous read lock, answered "not found" when the store cannot grant
//! one now, an edit under an asynchronous read-write lock, which runs at once
//! when the store is free and otherwise when its frame transaction lets it,
//! so a key the input method composed is never dropped.
//!
//! AppKit's input context keeps marked text of its own. When the field's
//! composition ends without the input method (the presentation completes it
//! for a press, focus moves, the application edits the composing text), the
//! context is told to forget it ([`InputContext::discard_marked_text`]), or
//! its next commit would insert the composed text a second time. When the
//! field's text moves on the screen, the context is told to ask again where
//! the candidate window goes ([`InputContext::invalidate_character_coordinates`]).
//!
//! Host operations never run under a call into a store: a request made while
//! one is on the stack (a lock grant's application code moving focus) waits
//! in the client's queue and runs when the outermost call returns.
//!
//! Pure Rust over the [`InputContext`] seam, so its rules run on every host;
//! the macOS backend (`text::apple::macos`) supplies the context.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Bounds, Point};
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{
    Composition, CompositionEnd, LockGrant, LockTiming, PointMode, Selection, TextChange,
    TextStore, TextStoreEdit, TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreRead,
    commit_composition_in_place,
};

use crate::owner::panic_boundary::contain_owner_callback;
use crate::text::host_queue::{HostQueue, MirrorHostOp};

#[cfg(test)]
mod tests;

/// What the client asks of AppKit's input context, the view's
/// `NSTextInputContext`.
pub(crate) trait InputContext {
    /// Forget the marked text the context holds (`discardMarkedText`). The
    /// input method may answer by committing or unmarking it through the
    /// client, inside this call.
    fn discard_marked_text(&self);

    /// The field's text moved: the candidate window asks for its rectangle
    /// again (`invalidateCharacterCoordinates`).
    fn invalidate_character_coordinates(&self);
}

/// One content view's text input: the store it serves and AppKit's context.
pub(crate) struct TextClient {
    /// The field the input method composes in; `None` when no field has focus.
    focused: RefCell<Option<Rc<dyn TextStore>>>,
    /// AppKit's input context; `None` until the view binds it and after it
    /// retires.
    context: RefCell<Option<Rc<dyn InputContext>>>,
    /// Whether AppKit's input context holds marked text it set through this
    /// client (`setMarkedText:` with text, until a commit, an unmark or a
    /// discard).
    marked: Cell<bool>,
    /// The requests made while the client is inside a call into a store.
    queue: HostQueue<MirrorHostOp>,
    retired: Cell<bool>,
    this: Weak<Self>,
}

impl std::fmt::Debug for TextClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextClient")
            .field("focused", &self.focused.borrow().is_some())
            .field("marked", &self.marked.get())
            .field("retired", &self.retired.get())
            .finish_non_exhaustive()
    }
}

/// `range` clamped into a document of `len` code units.
fn clamped(range: Utf16Range, len: Utf16Offset) -> Utf16Range {
    Utf16Range::try_new(range.start().min(len), range.end().min(len))
        .expect("BUG: clamping both ends keeps them ordered")
}

/// The span an edit replaces: AppKit's `replacementRange:` when it gives
/// one, else the composition, else the selection.
fn edit_target(session: &dyn TextStoreRead, replacement: Option<Utf16Range>) -> Utf16Range {
    let len = session.document_len();
    replacement.map_or_else(
        || {
            session.composition().map_or_else(
                || session.selection().range(),
                |composition| composition.range,
            )
        },
        |range| clamped(range, len),
    )
}

/// The span a query is about when AppKit names none: the composition, else
/// the selection.
fn query_target(session: &dyn TextStoreRead, range: Option<Utf16Range>) -> Utf16Range {
    range.map_or_else(
        || edit_target(session, None),
        |range| clamped(range, session.document_len()),
    )
}

/// `selected`, a range within `inserted` text that now spans `change`, as a
/// selection in the document, kept inside the inserted text.
fn selection_in(change: TextChange, selected: Utf16Range) -> Selection {
    let (start, end) = (change.start.get(), change.new_end.get());
    let anchor = start.saturating_add(selected.start().get()).min(end);
    let active = start.saturating_add(selected.end().get()).min(end);
    Selection {
        anchor: Utf16Offset::new(anchor),
        active: Utf16Offset::new(active),
    }
}

impl TextClient {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            focused: RefCell::new(None),
            context: RefCell::new(None),
            marked: Cell::new(false),
            queue: HostQueue::new(),
            retired: Cell::new(false),
            this: this.clone(),
        })
    }

    /// AppKit's input context for this view, once the view exists.
    pub(crate) fn bind(&self, context: Rc<dyn InputContext>) {
        if !self.retired.get() {
            *self.context.borrow_mut() = Some(context);
        }
    }

    /// The view is going away: the focused field is let go and AppKit's
    /// context is never reached again. A later call is a no-op.
    pub(crate) fn retire(&self) {
        if self.retired.replace(true) {
            return;
        }
        self.queue.clear();
        let context = self.context.borrow_mut().take();
        drop(context);
        let store = self.focused.borrow_mut().take();
        if let Some(store) = store {
            contain_owner_callback(|| store.set_observer(None));
        }
        self.marked.set(false);
    }

    /// The store the client serves now.
    fn focused(&self) -> Option<Rc<dyn TextStore>> {
        self.focused.borrow().clone()
    }

    /// AppKit's context, held for no borrow: its calls may come back in.
    fn context(&self) -> Option<Rc<dyn InputContext>> {
        self.context.borrow().clone()
    }

    /// Tell AppKit's context to forget its marked text, if it holds any, as
    /// a call into the focused store: what the input method does in answer
    /// (a commit, an unmark) edits the store under this call, and a host
    /// operation it causes waits for the call to return.
    fn discard_marked_text(&self) {
        if !self.marked.replace(false) {
            return;
        }
        if let Some(context) = self.context() {
            let _entered = self.queue.enter();
            context.discard_marked_text();
        }
    }

    fn invalidate_character_coordinates(&self) {
        if let Some(context) = self.context() {
            context.invalidate_character_coordinates();
        }
    }

    /// Apply the requests queued while the client was inside a store call.
    fn drain(&self) {
        while let Some(op) = self.queue.next() {
            self.run(op);
        }
    }

    fn run(&self, op: MirrorHostOp) {
        match op {
            MirrorHostOp::Focus(store) => self.refocus(store),
            MirrorHostOp::Complete(store) => {
                // Whoever was told `Deferred` owes nothing more: what AppKit
                // does not commit is committed in place here.
                let focused = self
                    .focused()
                    .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                let end = if focused {
                    self.end_composition(&store)
                } else {
                    CompositionEnd::Abandoned
                };
                if end != CompositionEnd::Committed {
                    let _ = commit_composition_in_place(&*store);
                }
            }
        }
    }

    fn refocus(&self, store: Option<Rc<dyn TextStore>>) {
        let same = match (&store, &self.focused()) {
            (Some(next), Some(current)) => Rc::ptr_eq(next, current),
            (None, None) => true,
            _ => false,
        };
        if same || self.retired.get() {
            return;
        }
        if let Some(previous) = self.focused() {
            // AppKit's composition belongs to the field losing focus: what
            // the input method makes of the discard reaches that field, and
            // what it leaves composing there is committed in place, so no
            // field keeps a composing range no input method owns.
            self.discard_marked_text();
            let _ = commit_composition_in_place(&*previous);
            *self.focused.borrow_mut() = None;
            contain_owner_callback(|| previous.set_observer(None));
        }
        if let Some(store) = store {
            let observer: Rc<dyn TextStoreObserver> = Rc::new(ClientObserver {
                client: self.this.clone(),
            });
            *self.focused.borrow_mut() = Some(Rc::clone(&store));
            contain_owner_callback(|| store.set_observer(Some(observer)));
        }
        self.invalidate_character_coordinates();
    }

    /// End AppKit's composition in `store`, the focused one: its context
    /// forgets its marked text, and the input method may commit it first.
    /// `Committed` when the store composes no more, `Abandoned` when the
    /// caller still has a composition to commit in place.
    fn end_composition(&self, store: &Rc<dyn TextStore>) -> CompositionEnd {
        self.discard_marked_text();
        match read(&**store, |session| session.composition().is_none()) {
            Some(true) => CompositionEnd::Committed,
            Some(false) | None => CompositionEnd::Abandoned,
        }
    }

    /// Read the focused store under a synchronous lock: `None` when no field
    /// has focus or the store cannot grant one now.
    fn read<R: 'static>(&self, body: impl FnOnce(&dyn TextStoreRead) -> R + 'static) -> Option<R> {
        let store = self.focused()?;
        let answer = {
            let _entered = self.queue.enter();
            read(&*store, body)
        };
        self.drain();
        answer
    }

    /// Edit the focused store under an asynchronous lock: now when it is
    /// free, else when its frame transaction lets it.
    fn edit(&self, body: impl FnOnce(&mut dyn TextStoreEdit) + 'static) {
        let Some(store) = self.focused() else {
            tracing::trace!("an input method edit with no focused field; dropped");
            return;
        };
        {
            let _entered = self.queue.enter();
            if let Err(error) = store.request_lock(LockGrant::read_write(body), LockTiming::Async) {
                tracing::debug!(?error, "the focused store refused the input method's edit");
            }
        }
        self.drain();
    }

    // ------------------------------------------------------------------
    // The view's NSTextInputClient verbs
    // ------------------------------------------------------------------

    /// Whether a key press goes to the input method: a field has focus.
    pub(crate) fn takes_key_presses(&self) -> bool {
        self.focused.borrow().is_some()
    }

    /// Whether a key release still belongs to the keyboard path: `false`
    /// exactly while AppKit's context holds marked text, since the press that
    /// opened the composition went to the input method.
    pub(crate) fn reports_key_release(&self) -> bool {
        !self.marked.get()
    }

    /// `insertText:replacementRange:` — `text` replaces `replacement`, else
    /// the composition, else the selection, and the composition ends.
    pub(crate) fn insert_text(&self, text: String, replacement: Option<Utf16Range>) {
        self.marked.set(false);
        self.edit(move |session| {
            let target = edit_target(session, replacement);
            if let Err(error) = session.replace(target, &text) {
                tracing::debug!(?error, "an input method commit did not fit the document");
            }
            if session.composition().is_some()
                && let Err(error) = session.set_composition(None)
            {
                tracing::debug!(?error, "a committed composition could not be cleared");
            }
        });
    }

    /// `setMarkedText:selectedRange:replacementRange:` — `text` replaces
    /// `replacement`, else the composition, else the selection, and becomes
    /// the composition; `selected` is the selection within `text`. An empty
    /// `text` ends the composition, leaving nothing in its place.
    pub(crate) fn set_marked_text(
        &self,
        text: String,
        selected: Utf16Range,
        replacement: Option<Utf16Range>,
    ) {
        self.marked.set(!text.is_empty());
        self.edit(move |session| {
            let target = edit_target(session, replacement);
            let change = match session.replace(target, &text) {
                Ok(change) => change,
                Err(error) => {
                    tracing::debug!(
                        ?error,
                        "an input method composition did not fit the document"
                    );
                    return;
                }
            };
            let composition =
                Utf16Range::try_new(change.start, change.new_end).map(|range| Composition {
                    range,
                    hides_caret: false,
                });
            let composition = composition.filter(|_| !text.is_empty());
            if let Err(error) = session.set_composition(composition) {
                tracing::debug!(?error, "an input method composition could not be marked");
            }
            if let Err(error) = session.set_selection(selection_in(change, selected)) {
                tracing::debug!(?error, "the selection inside a composition was refused");
            }
        });
    }

    /// `unmarkText` — the composition ends, keeping its text.
    pub(crate) fn unmark_text(&self) {
        self.marked.set(false);
        self.edit(|session| {
            if session.composition().is_some()
                && let Err(error) = session.set_composition(None)
            {
                tracing::debug!(?error, "an unmarked composition could not be cleared");
            }
        });
    }

    /// `markedRange` — the composition's span; `None` when nothing composes.
    pub(crate) fn marked_range(&self) -> Option<Utf16Range> {
        self.read(|session| session.composition().map(|composition| composition.range))
            .flatten()
    }

    /// `selectedRange` — the selection's span; `None` when no field has focus.
    pub(crate) fn selected_range(&self) -> Option<Utf16Range> {
        self.read(|session| session.selection().range())
    }

    /// `attributedSubstringForProposedRange:actualRange:` — the text of
    /// `range` clamped to the document, and the range it covers; `None` for a
    /// protected field or a range that splits a character.
    pub(crate) fn substring(&self, range: Utf16Range) -> Option<(String, Utf16Range)> {
        self.read(move |session| {
            let range = clamped(range, session.document_len());
            session.text(range).ok().map(|text| (text, range))
        })
        .flatten()
    }

    /// `firstRectForCharacterRange:actualRange:` — the rectangle of `range`
    /// (the composition, else the selection, when AppKit names none) in the
    /// window's logical coordinates, and the range it covers. One line: the
    /// whole range is its first line.
    pub(crate) fn first_rect(
        &self,
        range: Option<Utf16Range>,
    ) -> Option<(Bounds<f64>, Utf16Range)> {
        self.read(move |session| {
            let range = query_target(session, range);
            session
                .rect_for_range(range)
                .ok()
                .map(|rect| (rect.bounds, range))
        })
        .flatten()
    }

    /// `characterIndexForPoint:` — the character boundary nearest `point`,
    /// in the window's logical coordinates.
    pub(crate) fn character_index(&self, point: Point<f64>) -> Option<Utf16Offset> {
        self.read(move |session| session.index_at_point(point, PointMode::NEAREST).ok())
            .flatten()
    }

    // ------------------------------------------------------------------
    // What the field's store reports
    // ------------------------------------------------------------------

    /// The application changed the field: a composition its edit ended is
    /// one AppKit's context must forget.
    fn application_edited(&self) {
        if !self.marked.get() {
            return;
        }
        let composing = self.read(|session| session.composition().is_some());
        if composing == Some(false) {
            self.discard_marked_text();
            self.drain();
        }
    }
}

/// Read `store` under a synchronous lock: `None` when it cannot grant one now.
fn read<R: 'static>(
    store: &dyn TextStore,
    body: impl FnOnce(&dyn TextStoreRead) -> R + 'static,
) -> Option<R> {
    let answer: Rc<RefCell<Option<R>>> = Rc::default();
    let out = Rc::clone(&answer);
    let grant = LockGrant::read(move |session| {
        *out.borrow_mut() = Some(body(session));
    });
    // A synchronous lock runs before the request returns or is refused.
    let _granted = store.request_lock(grant, LockTiming::Sync).ok()?;
    answer.take()
}

impl TextStoreHost for TextClient {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        if let Some(op) = self.queue.defer(MirrorHostOp::Focus(store)) {
            self.run(op);
            self.drain();
        }
    }

    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        if self.retired.get() {
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::NoWindow,
            )
            .into());
        }
        // The focus a queued request will give is the one asked about.
        let focused = self.queue.focus_after_queue(|| self.focused());
        if !focused.is_some_and(|focused| Rc::ptr_eq(&focused, store)) {
            return Err(TextStoreHostError::NotFocused);
        }
        if self
            .queue
            .defer(MirrorHostOp::Complete(Rc::clone(store)))
            .is_none()
        {
            return Ok(CompositionEnd::Deferred);
        }
        let end = self.end_composition(store);
        self.drain();
        Ok(end)
    }
}

/// What the focused store tells the client when the application changes the
/// field.
struct ClientObserver {
    client: Weak<TextClient>,
}

impl TextStoreObserver for ClientObserver {
    fn text_changed(&self, _change: TextChange) {
        if let Some(client) = self.client.upgrade() {
            client.application_edited();
        }
    }

    fn selection_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.application_edited();
        }
    }

    fn layout_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.invalidate_character_coordinates();
        }
    }

    fn status_changed(&self) {}
}
