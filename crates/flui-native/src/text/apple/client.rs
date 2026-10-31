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
    TextStoreStatus, commit_composition_in_place,
};

use crate::owner::panic_boundary::contain_owner_callback;
use crate::text::host_queue::{HostQueue, MirrorHostOp};

#[cfg(test)]
mod tests;

/// What the client asks of the platform's input method: AppKit's
/// `NSTextInputContext` on macOS, UIKit's keyboard through the responder's
/// `inputDelegate` on iOS.
pub(crate) trait InputContext {
    /// Forget the marked text the input method holds (`discardMarkedText`).
    /// The input method may answer by committing or unmarking it through the
    /// client, inside this call.
    fn discard_marked_text(&self);

    /// The field's text moved: the candidate window asks for its rectangle
    /// again (`invalidateCharacterCoordinates`).
    fn invalidate_character_coordinates(&self);

    /// The field the input method serves, by its status, or `None` when no
    /// field has focus or the client retired. Called on every change that
    /// can change the answer (focus, the focused field's status, binding,
    /// retirement, the window's key status) and possibly with the same
    /// answer again: the context applies only what changed.
    fn field_changed(&self, status: Option<TextStoreStatus>);

    /// The application changed the focused field's document, `change`, which
    /// the input method did not make: never called for an edit it made
    /// through the client, which it already knows of. The input method may
    /// answer through the client, inside this call.
    fn document_changed(&self, change: DocumentChange);
}

/// What the application changed of the focused field's document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentChange {
    /// Its text.
    Text,
    /// Its selection alone.
    Selection,
}

/// What [`TextClient::type_text`] made of the keyboard's text.
#[cfg(any(target_os = "ios", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Typed {
    /// The Return key: the caller sends the field an Enter press.
    Return,
    /// The text went into the field.
    Inserted,
}

/// One content view's text input: the store it serves and AppKit's context.
pub(crate) struct TextClient {
    /// The field the input method composes in; `None` when no field has focus.
    focused: RefCell<Option<Rc<dyn TextStore>>>,
    /// The input method's context; `None` until the view binds it and after it
    /// retires.
    context: RefCell<Option<Rc<dyn InputContext>>>,
    /// Whether the input method holds marked text it set through this
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

    /// The input method's context for this view, once the view exists.
    pub(crate) fn bind(&self, context: Rc<dyn InputContext>) {
        if !self.retired.get() {
            *self.context.borrow_mut() = Some(context);
            // A field may have taken focus before the view existed.
            self.sync_field();
        }
    }

    /// The view is going away: the focused field is let go and the input method's
    /// context is never reached again. A later call is a no-op.
    pub(crate) fn retire(&self) {
        if self.retired.replace(true) {
            return;
        }
        self.queue.clear();
        // Retired, the client serves no field: what the context took for
        // one (a secure input hold, the keyboard) is given back while it is
        // still there to give it back through.
        self.sync_field();
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

    /// Tell the input method which field it serves, by its status: what it
    /// does with it (a secure keyboard, secure event input, the keyboard's
    /// traits) follows the field. Called wherever the answer can change:
    /// focus, the focused field's status, the window's key status, binding
    /// and retirement.
    fn sync_field(&self) {
        let status = {
            // The store's `status` is application code, which may move
            // focus: a request it makes waits in the queue, and the refocus
            // the drain below runs syncs again with the focus it gives.
            let _entered = self.queue.enter();
            if self.retired.get() {
                None
            } else {
                self.focused().map(|store| store.status())
            }
        };
        // The input method's answer may come back into the client: no
        // borrow is held.
        if let Some(context) = self.context() {
            context.field_changed(status);
        }
        self.drain();
    }

    /// The view's window became or stopped being the key window: the keys
    /// typed reach this window's field only while it is.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn key_window_changed(&self) {
        self.sync_field();
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
        self.sync_field();
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
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn takes_key_presses(&self) -> bool {
        self.focused.borrow().is_some()
    }

    /// Whether a key release still belongs to the keyboard path: `false`
    /// exactly while AppKit's context holds marked text, since the press that
    /// opened the composition went to the input method.
    #[cfg(any(target_os = "macos", test))]
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
    // UIKit's UITextInput verbs beyond AppKit's
    // ------------------------------------------------------------------

    /// `replaceRange:withText:` — `text` replaces `range`, clamped to the
    /// document (an autocorrection, a text shortcut); the composition, if
    /// any, stays as the store keeps it.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn replace(&self, range: Utf16Range, text: String) {
        self.edit(move |session| {
            let range = clamped(range, session.document_len());
            if let Err(error) = session.replace(range, &text) {
                tracing::debug!(
                    ?error,
                    "an input method replacement did not fit the document"
                );
            }
        });
    }

    /// `setSelectedTextRange:` — the selection becomes `range`, clamped to
    /// the document, its active end at `range`'s end.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn select(&self, range: Utf16Range) {
        self.edit(move |session| {
            let range = clamped(range, session.document_len());
            let selection = Selection {
                anchor: range.start(),
                active: range.end(),
            };
            if let Err(error) = session.set_selection(selection) {
                tracing::debug!(?error, "the input method's selection was refused");
            }
        });
    }

    /// `endOfDocument` — the document's length in UTF-16 units; `None` when
    /// no field has focus or the store cannot grant a read now.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn document_len(&self) -> Option<Utf16Offset> {
        self.read(|session| session.document_len())
    }

    /// `insertText:` from UIKit's keyboard: a lone line break is the Return
    /// key, which the field answers as a key press (a single-line field
    /// submits), as AppKit's `insertNewline:` and TSF send it; any other
    /// text, a line break inside dictated text included, is inserted as
    /// [`insert_text`](Self::insert_text) inserts it.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn type_text(&self, text: String) -> Typed {
        if text == "\n" {
            return Typed::Return;
        }
        self.insert_text(text, None);
        Typed::Inserted
    }

    // ------------------------------------------------------------------
    // What the field's store reports
    // ------------------------------------------------------------------

    /// The application changed the field, `change`: a composition its edit
    /// ended is one the input method must forget, and the input method is
    /// told of the change, which it did not make.
    fn application_edited(&self, change: DocumentChange) {
        if self.marked.get() {
            let composing = self.read(|session| session.composition().is_some());
            if composing == Some(false) {
                self.discard_marked_text();
            }
        }
        if let Some(context) = self.context() {
            // As a call into the focused store: what the input method does
            // in answer edits the store under it, and a host operation it
            // causes waits for it to return.
            let _entered = self.queue.enter();
            context.document_changed(change);
        }
        self.drain();
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
            client.application_edited(DocumentChange::Text);
        }
    }

    fn selection_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.application_edited(DocumentChange::Selection);
        }
    }

    fn layout_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.invalidate_character_coordinates();
        }
    }

    fn status_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.sync_field();
        }
    }
}
