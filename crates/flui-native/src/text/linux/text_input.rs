//! The focused field's document as a Wayland input method reaches it through
//! `zwp_text_input_v3`: the protocol's state and events over the field's
//! [`TextStore`], and the window's [`TextStoreHost`].
//!
//! The protocol is double-buffered both ways. The client describes the field
//! (its surrounding text with the cursor and anchor in UTF-8 bytes, its
//! content type, the caret's rectangle) and makes that state current with a
//! `commit`. The compositor answers with a preedit, a committed string and a
//! deletion around the cursor, all made current by a `done` that carries the
//! number of commits it had seen. The client applies a `done` in the
//! protocol's order:
//!
//! 1. the composition the last `done` set is removed;
//! 2. the text around the selection is deleted;
//! 3. the committed string replaces the selection;
//! 4. the new preedit is inserted as the composition, with the caret where
//!    its cursor says, or hidden.
//!
//! A preedit, a commit and a deletion are each reset by the `done` that
//! applies them, so a `done` that names no preedit clears it. The state is
//! sent again after a `done` only when that `done` counted every commit sent,
//! and otherwise waits for one that does, as the protocol requires.
//!
//! Every edit runs under an asynchronous read-write lock: at once when the
//! store is free, else when its frame transaction lets it, so a string the
//! input method committed is never dropped. Host operations never run under
//! a call into a store: a request made while one is on the stack (a lock
//! grant's application code moving focus) waits in the client's queue.
//!
//! Pure Rust over the [`TextInputWire`] seam, so its rules run on every
//! host; the Wayland backend (`text::linux::wayland`) supplies the wire.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::Bounds;
use flui_platform::text_store::utf16::{self, Utf16Offset, Utf16Range};
use flui_platform::text_store::{
    Composition, CompositionEnd, LockGrant, LockOutcome, LockTiming, Selection, TextChange,
    TextInputPurpose, TextStore, TextStoreEdit, TextStoreHost, TextStoreHostError,
    TextStoreObserver, TextStoreRead,
};

use crate::owner::panic_boundary::contain_owner_callback;
use crate::text::host_queue::{HostQueue, MirrorHostOp};

#[cfg(test)]
mod tests;

/// The most bytes of surrounding text one request carries: the protocol
/// caps the whole request at 4000 bytes, its header included.
pub(crate) const MAX_SURROUNDING_BYTES: usize = 3900;

/// Why the surrounding text changed (`zwp_text_input_v3.change_cause`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChangeCause {
    /// The input method's own edit, applied at a `done`.
    InputMethod,
    /// Anything else: the application, the user's selection, a focus change.
    Other,
}

/// The field's content type in the protocol's terms: the hint bits and the
/// purpose (`zwp_text_input_v3.content_hint`, `.content_purpose`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContentType {
    /// `content_hint` bits.
    pub(crate) hint: u32,
    /// `content_purpose`.
    pub(crate) purpose: u32,
}

impl ContentType {
    /// `content_hint.multiline`.
    pub(crate) const HINT_MULTILINE: u32 = 0x200;
    /// `content_hint.hidden_text`.
    pub(crate) const HINT_HIDDEN_TEXT: u32 = 0x40;
    /// `content_hint.sensitive_data`.
    pub(crate) const HINT_SENSITIVE_DATA: u32 = 0x80;

    /// `content_purpose.normal`.
    pub(crate) const NORMAL: u32 = 0;
    /// `content_purpose.number`.
    pub(crate) const NUMBER: u32 = 3;
    /// `content_purpose.phone`.
    pub(crate) const PHONE: u32 = 4;
    /// `content_purpose.url`.
    pub(crate) const URL: u32 = 5;
    /// `content_purpose.email`.
    pub(crate) const EMAIL: u32 = 6;
    /// `content_purpose.password`.
    pub(crate) const PASSWORD: u32 = 8;

    /// What a field of `purpose` tells the input method. A protected field
    /// is a password whose text the input method neither shows nor learns;
    /// a search field composes as free text, as the protocol has no search
    /// purpose.
    pub(crate) fn of(purpose: TextInputPurpose, protected: bool, single_line: bool) -> Self {
        let mut hint = if single_line { 0 } else { Self::HINT_MULTILINE };
        let purpose = if protected {
            hint |= Self::HINT_HIDDEN_TEXT | Self::HINT_SENSITIVE_DATA;
            Self::PASSWORD
        } else {
            match purpose {
                TextInputPurpose::Email => Self::EMAIL,
                TextInputPurpose::Url => Self::URL,
                TextInputPurpose::Phone => Self::PHONE,
                TextInputPurpose::Number => Self::NUMBER,
                // Free text, a search, and a purpose added after this table.
                _ => Self::NORMAL,
            }
        };
        Self { hint, purpose }
    }

    /// What `store`'s field tells the input method.
    pub(crate) fn of_store(store: &dyn TextStore) -> Self {
        let status = store.status();
        Self::of(status.purpose, status.protected, status.single_line)
    }
}

/// What the client sends on a seat's `zwp_text_input_v3`.
///
/// The backend's own code: no call runs application code or comes back into
/// the client, so the client calls it under a lock grant.
pub(crate) trait TextInputWire {
    /// `enable`: the field takes text input. Resets the input method's state.
    fn enable(&self);
    /// `disable`: no field takes text input.
    fn disable(&self);
    /// `set_surrounding_text`, with `cursor` and `anchor` in bytes of `text`.
    fn set_surrounding_text(&self, text: &str, cursor: usize, anchor: usize);
    /// `set_text_change_cause`.
    fn set_text_change_cause(&self, cause: ChangeCause);
    /// `set_content_type`.
    fn set_content_type(&self, content: ContentType);
    /// `set_cursor_rectangle`, from `rect` in the window's logical
    /// coordinates; the wire places it on the surface.
    fn set_cursor_rectangle(&self, rect: Bounds<f64>);
    /// `commit`: what was sent since becomes current.
    fn commit(&self);
    /// How many `commit`s were sent on this object.
    fn commits(&self) -> u32;
}

/// The surrounding text the client sends: a window of the document around
/// the selection, the composition left out, with the selection's ends in
/// its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Surrounding {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) anchor: usize,
}

/// The text of `start..end`, narrowed by a code unit at an end that splits a
/// surrogate pair; `None` for a protected field.
fn text_within(
    session: &dyn TextStoreRead,
    start: usize,
    end: usize,
) -> Option<(String, Utf16Offset)> {
    for (start, end) in [
        (start, end),
        (start + 1, end),
        (start, end.saturating_sub(1)),
        (start + 1, end.saturating_sub(1)),
    ] {
        let Some(range) = Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end))
        else {
            return Some((String::new(), Utf16Offset::new(start.min(end))));
        };
        match session.text(range) {
            Ok(text) => return Some((text, range.start())),
            Err(flui_platform::text_store::TextStoreError::Protected) => return None,
            Err(_) => {}
        }
    }
    None
}

/// The longest tail of `text` within `budget` bytes, cut at a `char`.
fn tail(text: &str, budget: usize) -> &str {
    let mut from = text.len().saturating_sub(budget);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    &text[from..]
}

/// The longest head of `text` within `budget` bytes, cut at a `char`.
fn head(text: &str, budget: usize) -> &str {
    let mut to = budget.min(text.len());
    while !text.is_char_boundary(to) {
        to -= 1;
    }
    &text[..to]
}

/// The selection as the input method is told it, with its text: whole when
/// its text fits the surrounding budget, else a caret at its active end. A
/// deletion around the selection counts from the same selection, so what the
/// input method deletes is what it saw. `None` for a protected field.
fn told_selection(session: &dyn TextStoreRead) -> Option<(Selection, String)> {
    let selection = session.selection();
    let range = selection.range();
    let (selected, _) = text_within(session, range.start().get(), range.end().get())?;
    if selected.len() > MAX_SURROUNDING_BYTES {
        Some((Selection::collapsed(selection.active), String::new()))
    } else {
        Some((selection, selected))
    }
}

/// The surrounding text of the document `session` reads, or `None` for a
/// protected field, whose text the input method never sees.
///
/// While a composition is open it is left out and the cursor sits where it
/// was; otherwise the selection is kept whole when it fits and the rest of
/// the budget goes to the text on either side, half each, the unused half
/// of one side going to the other. A selection that does not fit is sent as
/// a caret at its active end. A NUL, which a Wayland string cannot carry, is
/// sent as a space: one byte for one, so every offset still names the
/// document's own byte.
pub(crate) fn surrounding(session: &dyn TextStoreRead) -> Option<Surrounding> {
    let len = session.document_len().get();
    // A code unit is at least one byte: `budget` units hold every byte the
    // budget can take.
    let budget = MAX_SURROUNDING_BYTES;
    let (lo, hi, selected, forward) = if let Some(composition) = session.composition() {
        let range = composition.range;
        (range.start().get(), range.end().get(), String::new(), true)
    } else {
        let (selection, selected) = told_selection(session)?;
        let range = selection.range();
        (
            range.start().get(),
            range.end().get(),
            selected,
            selection.anchor <= selection.active,
        )
    };
    let (before, _) = text_within(session, lo.saturating_sub(budget), lo)?;
    let (after, _) = text_within(session, hi, hi.saturating_add(budget).min(len))?;
    let room = budget - selected.len();
    let after_wanted = head(&after, room / 2).len();
    let before = tail(&before, room - after_wanted);
    let after = head(&after, room - before.len());
    let start = before.len();
    let end = start + selected.len();
    let (anchor, cursor) = if forward { (start, end) } else { (end, start) };
    Some(Surrounding {
        text: format!("{before}{selected}{after}").replace('\0', " "),
        cursor,
        anchor,
    })
}

/// The span `before` bytes before `range` and `after` bytes after it cover,
/// whole characters only: a count that ends inside a character stops short
/// of it. `None` for a protected field.
pub(crate) fn surrounding_deletion(
    session: &dyn TextStoreRead,
    range: Utf16Range,
    before: usize,
    after: usize,
) -> Option<(Utf16Range, Utf16Range)> {
    let (lo, hi) = (range.start().get(), range.end().get());
    let len = session.document_len().get();
    let (text, _) = text_within(session, lo.saturating_sub(before), lo)?;
    let kept = tail(&text, before);
    let gone = utf16::utf16_len(kept).get();
    let before_span = Utf16Range::try_new(Utf16Offset::new(lo - gone), Utf16Offset::new(lo))
        .expect("BUG: a span ending at the selection starts before it");
    let (text, _) = text_within(session, hi, hi.saturating_add(after).min(len))?;
    let kept = head(&text, after);
    let gone = utf16::utf16_len(kept).get();
    let after_span = Utf16Range::try_new(Utf16Offset::new(hi), Utf16Offset::new(hi + gone))
        .expect("BUG: a span starting at the selection ends after it");
    Some((before_span, after_span))
}

/// A preedit as `preedit_string` gives it: its text and its cursor, a byte
/// range of the text, or `None` to hide the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Preedit {
    pub(crate) text: String,
    pub(crate) cursor: Option<(usize, usize)>,
}

impl Preedit {
    /// From `preedit_string`'s arguments: a negative cursor hides the caret,
    /// and one past the text or splitting a character is taken as its end.
    pub(crate) fn new(text: String, begin: i32, end: i32) -> Self {
        let place = |at: i32| {
            let mut at = usize::try_from(at).ok()?.min(text.len());
            while !text.is_char_boundary(at) {
                at -= 1;
            }
            Some(at)
        };
        let cursor = place(begin)
            .zip(place(end))
            .map(|(begin, end)| (begin.min(end), begin.max(end)));
        Self { text, cursor }
    }
}

/// What the compositor sent since the last `done`, applied at the next.
#[derive(Debug, Default)]
struct Pending {
    preedit: Option<Preedit>,
    commit: Option<String>,
    delete: Option<(u32, u32)>,
}

/// One window's text input over `zwp_text_input_v3`: the store it serves and
/// the wire of the seat whose text input is in the window.
pub(crate) struct TextInputClient {
    /// The field the input method composes in; `None` when no field has focus.
    focused: RefCell<Option<Rc<dyn TextStore>>>,
    /// The text input of the seat in this window; `None` while none is.
    wire: RefCell<Option<Rc<dyn TextInputWire>>>,
    /// Whether `enable` was sent on the wire since it entered.
    enabled: Cell<bool>,
    /// Counts each `enable`: a grant made under one acts on the wire only
    /// while the same one holds, never on the field that took focus since.
    session: Cell<u64>,
    pending: RefCell<Pending>,
    /// The requests made while the client is inside a call into a store.
    queue: HostQueue<MirrorHostOp>,
    retired: Cell<bool>,
    this: Weak<Self>,
}

impl std::fmt::Debug for TextInputClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputClient")
            .field("focused", &self.focused.borrow().is_some())
            .field("entered", &self.wire.borrow().is_some())
            .field("enabled", &self.enabled.get())
            .field("retired", &self.retired.get())
            .finish_non_exhaustive()
    }
}

/// Send the field's state on `wire`, read from `session`: the content type,
/// the surrounding text (none for a protected field) and the caret's
/// rectangle, then commit.
fn send_state(wire: &dyn TextInputWire, session: &dyn TextStoreRead, cause: ChangeCause) {
    let sent = surrounding(session);
    if let Some(surrounding) = &sent {
        wire.set_surrounding_text(&surrounding.text, surrounding.cursor, surrounding.anchor);
        wire.set_text_change_cause(cause);
    }
    send_caret(wire, session);
    wire.commit();
}

/// Send the caret's rectangle: the composition's, else the selection's.
fn send_caret(wire: &dyn TextInputWire, session: &dyn TextStoreRead) {
    let range = session.composition().map_or_else(
        || Utf16Range::collapsed(session.selection().active),
        |composition| composition.range,
    );
    if let Ok(rect) = session.rect_for_range(range) {
        wire.set_cursor_rectangle(rect.bounds);
    }
}

/// Apply one `done`'s edits in the protocol's order (see the module).
fn apply(session: &mut dyn TextStoreEdit, pending: Pending) {
    if let Some(composition) = session.composition() {
        if let Err(error) = session.replace(composition.range, "") {
            tracing::debug!(?error, "the last preedit could not be removed");
        }
        if session.composition().is_some()
            && let Err(error) = session.set_composition(None)
        {
            tracing::debug!(?error, "the last preedit could not be ended");
        }
    }
    if let Some((before, after)) = pending.delete
        && let Some((told, _)) = told_selection(session)
    {
        let range = told.range();
        let spans = surrounding_deletion(
            session,
            range,
            usize::try_from(before).unwrap_or(usize::MAX),
            usize::try_from(after).unwrap_or(usize::MAX),
        );
        if let Some((before, after)) = spans {
            // The later span first, so the earlier one's offsets hold.
            for span in [after, before] {
                if !span.is_empty()
                    && let Err(error) = session.replace(span, "")
                {
                    tracing::debug!(?error, "text around the cursor could not be deleted");
                }
            }
            let caret = Utf16Offset::new(before.start().get());
            let selection = Selection {
                anchor: caret,
                active: Utf16Offset::new(caret.get() + range.len()),
            };
            if let Err(error) = session.set_selection(selection) {
                tracing::debug!(?error, "the selection after a deletion was refused");
            }
        }
    }
    if let Some(text) = pending.commit
        && let Err(error) = session.insert_at_selection(&text)
    {
        tracing::debug!(?error, "an input method commit did not fit the document");
    }
    if let Some(preedit) = pending.preedit.filter(|preedit| !preedit.text.is_empty()) {
        let change = match session.insert_at_selection(&preedit.text) {
            Ok(change) => change,
            Err(error) => {
                tracing::debug!(?error, "a preedit did not fit the document");
                return;
            }
        };
        let Some(range) = Utf16Range::try_new(change.start, change.new_end) else {
            return;
        };
        let composition = Composition {
            range,
            hides_caret: preedit.cursor.is_none(),
        };
        if let Err(error) = session.set_composition(Some(composition)) {
            tracing::debug!(?error, "a preedit could not be marked");
        }
        let selection = preedit.cursor.map_or_else(
            || Selection::collapsed(change.new_end),
            |(begin, end)| caret_in(&preedit.text, change, begin, end),
        );
        if let Err(error) = session.set_selection(selection) {
            tracing::debug!(?error, "the caret inside a preedit was refused");
        }
    }
}

/// The selection inside a preedit that now spans `change`, from its cursor's
/// byte range.
fn caret_in(text: &str, change: TextChange, begin: usize, end: usize) -> Selection {
    let at = |byte: usize| {
        let units = utf16::utf16_offset(text, byte).map_or(0, Utf16Offset::get);
        Utf16Offset::new((change.start.get() + units).min(change.new_end.get()))
    };
    Selection {
        anchor: at(begin),
        active: at(end),
    }
}

impl TextInputClient {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            focused: RefCell::new(None),
            wire: RefCell::new(None),
            enabled: Cell::new(false),
            session: Cell::new(0),
            pending: RefCell::new(Pending::default()),
            queue: HostQueue::new(),
            retired: Cell::new(false),
            this: this.clone(),
        })
    }

    /// The window is going away: the focused field is let go and the wire is
    /// never reached again. A later call is a no-op.
    pub(crate) fn retire(&self) {
        if self.retired.replace(true) {
            return;
        }
        self.queue.clear();
        if self.enabled.replace(false)
            && let Some(wire) = self.wire()
        {
            wire.disable();
            wire.commit();
        }
        let wire = self.wire.borrow_mut().take();
        drop(wire);
        let store = self.focused.borrow_mut().take();
        if let Some(store) = store {
            contain_owner_callback(|| store.set_observer(None));
        }
    }

    fn focused(&self) -> Option<Rc<dyn TextStore>> {
        self.focused.borrow().clone()
    }

    fn wire(&self) -> Option<Rc<dyn TextInputWire>> {
        self.wire.borrow().clone()
    }

    // ------------------------------------------------------------------
    // What the seat's text input reports
    // ------------------------------------------------------------------

    /// `enter`: the seat's text input is in this window. The field with focus,
    /// if any, takes text input.
    pub(crate) fn entered(&self, wire: Rc<dyn TextInputWire>) {
        if self.retired.get() {
            return;
        }
        let previous = self.wire.borrow_mut().replace(wire);
        // Another seat's text input served the window: its input method is
        // let go first, so it neither keeps the field nor hears its keys.
        if self.enabled.get()
            && let Some(previous) = previous
        {
            previous.disable();
            previous.commit();
        }
        self.enabled.set(false);
        *self.pending.borrow_mut() = Pending::default();
        self.sync_enablement();
    }

    /// Whether `wire` is the text input this window serves: the last seat's
    /// to enter. Another seat's events in the window are not this client's.
    fn serves(&self, wire: &dyn TextInputWire) -> bool {
        self.wire
            .borrow()
            .as_ref()
            .is_some_and(|current| std::ptr::addr_eq(Rc::as_ptr(current), std::ptr::from_ref(wire)))
    }

    /// `leave` of `wire`: the seat's text input left this window. The
    /// preedit is reset, as the protocol asks: its text leaves the field. A
    /// seat's text input the window does not serve leaves nothing behind.
    /// Whether `wire` was the one served.
    pub(crate) fn left(&self, wire: &dyn TextInputWire) -> bool {
        if !self.serves(wire) {
            return false;
        }
        let wire = self.wire.borrow_mut().take();
        drop(wire);
        self.enabled.set(false);
        *self.pending.borrow_mut() = Pending::default();
        self.edit(|session| apply(session, Pending::default()));
        true
    }

    /// `preedit_string` on `wire`.
    pub(crate) fn preedit(
        &self,
        wire: &dyn TextInputWire,
        text: Option<String>,
        begin: i32,
        end: i32,
    ) {
        if self.serves(wire) {
            self.pending.borrow_mut().preedit = text.map(|text| Preedit::new(text, begin, end));
        }
    }

    /// `commit_string` on `wire`.
    pub(crate) fn commit_string(&self, wire: &dyn TextInputWire, text: Option<String>) {
        if self.serves(wire) {
            self.pending.borrow_mut().commit = text;
        }
    }

    /// `delete_surrounding_text` on `wire`, in bytes before and after the
    /// selection.
    pub(crate) fn delete_surrounding(&self, wire: &dyn TextInputWire, before: u32, after: u32) {
        if self.serves(wire) {
            self.pending.borrow_mut().delete = Some((before, after));
        }
    }

    /// `done` on `wire`: apply what was sent since the last one, and send the
    /// field's state again when `serial` counted every commit sent as the
    /// `done` came. An edit the store holds until its frame ends keeps that
    /// answer: the commits sent by an earlier held edit's state do not make
    /// this one's stale, so the last edit's state is the one the input
    /// method holds.
    pub(crate) fn done(&self, wire: &dyn TextInputWire, serial: u32) {
        if !self.serves(wire) {
            return;
        }
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        if !self.enabled.get() {
            return;
        }
        let current = wire.commits() == serial;
        let session = self.session.get();
        let this = self.this.clone();
        self.edit(move |edit| {
            apply(edit, pending);
            let Some(client) = this.upgrade() else {
                return;
            };
            if current && let Some(wire) = client.wire_of(session) {
                send_state(&*wire, edit, ChangeCause::InputMethod);
            }
        });
    }

    /// The wire while the `enable` counted `session` holds: the field it
    /// enabled still has focus and the seat is still in the window.
    fn wire_of(&self, session: u64) -> Option<Rc<dyn TextInputWire>> {
        self.wire()
            .filter(|_| self.enabled.get() && self.session.get() == session)
    }

    // ------------------------------------------------------------------
    // Enablement and state
    // ------------------------------------------------------------------

    /// Enable or disable the wire as a field with focus and a seat in the
    /// window allow, sending the field's state as it is enabled.
    fn sync_enablement(&self) {
        let Some(wire) = self.wire() else {
            return;
        };
        let store = self.focused().filter(|_| !self.retired.get());
        match (store, self.enabled.get()) {
            (Some(store), false) => {
                self.enabled.set(true);
                self.enable_on(&wire, &store);
            }
            (None, true) => {
                self.enabled.set(false);
                wire.disable();
                wire.commit();
            }
            _ => {}
        }
    }

    /// Send `enable` and the state of `store` on `wire`.
    fn enable_on(&self, wire: &Rc<dyn TextInputWire>, store: &Rc<dyn TextStore>) {
        self.session.set(self.session.get().wrapping_add(1));
        wire.enable();
        wire.set_content_type(ContentType::of_store(&**store));
        let sent = {
            let wire = Rc::clone(wire);
            self.read(move |session| send_state(&*wire, session, ChangeCause::Other))
        };
        if sent.is_none() {
            // The store could not be read now: the input method starts from
            // an empty field, and the next change tells it the text.
            wire.commit();
        }
    }

    /// The field changed outside the input method: tell it, while enabled.
    fn resend_state(&self) {
        if !self.enabled.get() {
            return;
        }
        let Some(wire) = self.wire() else {
            return;
        };
        let _ = self.read(move |session| send_state(&*wire, session, ChangeCause::Other));
    }

    /// The field moved on the screen: tell the input method where the caret
    /// is now, while enabled.
    fn resend_caret(&self) {
        if !self.enabled.get() {
            return;
        }
        let Some(wire) = self.wire() else {
            return;
        };
        let _ = self.read(move |session| {
            send_caret(&*wire, session);
            wire.commit();
        });
    }

    // ------------------------------------------------------------------
    // Host operations
    // ------------------------------------------------------------------

    fn drain(&self) {
        while let Some(op) = self.queue.next() {
            self.run(op);
        }
    }

    fn run(&self, op: MirrorHostOp) {
        match op {
            MirrorHostOp::Focus(store) => self.refocus(store),
            MirrorHostOp::Complete(store) => {
                let focused = self
                    .focused()
                    .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                let end = if focused {
                    self.end_composition(&store)
                } else {
                    CompositionEnd::Abandoned
                };
                if end != CompositionEnd::Committed {
                    let _ = flui_platform::text_store::commit_composition_in_place(&*store);
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
            // The input method's composition belongs to the field losing
            // focus: it is committed there, and the input method is reset by
            // the next `enable` or told no field takes text.
            let _ = flui_platform::text_store::commit_composition_in_place(&*previous);
            *self.focused.borrow_mut() = None;
            contain_owner_callback(|| previous.set_observer(None));
            if self.enabled.replace(false)
                && let Some(wire) = self.wire()
            {
                wire.disable();
                wire.commit();
            }
        }
        if let Some(store) = store {
            let observer: Rc<dyn TextStoreObserver> = Rc::new(ClientObserver {
                client: self.this.clone(),
            });
            *self.focused.borrow_mut() = Some(Rc::clone(&store));
            contain_owner_callback(|| store.set_observer(Some(observer)));
        }
        self.sync_enablement();
    }

    /// End the input method's composition in `store`, the focused one: the
    /// preedit stays as the field's text, and the input method, reset by a
    /// fresh `enable`, forgets it. `Committed` when that ran now, `Abandoned`
    /// when the store queued it and the caller commits in place too.
    fn end_composition(&self, store: &Rc<dyn TextStore>) -> CompositionEnd {
        let this = self.this.clone();
        // `store` has focus now: a grant the store holds acts on the wire
        // only while the same `enable` holds, so a field focused since is
        // never reset or told this one's text.
        let session = self.session.get();
        let content = ContentType::of_store(&**store);
        let grant = LockGrant::read_write(move |edit| {
            if edit.composition().is_some()
                && let Err(error) = edit.set_composition(None)
            {
                tracing::debug!(?error, "a completed preedit could not be ended");
            }
            let Some(client) = this.upgrade() else {
                return;
            };
            if let Some(wire) = client.wire_of(session) {
                // `enable` resets the content type too: it is sent again
                // with the rest of the state, as after any `enable`.
                wire.enable();
                wire.set_content_type(content);
                send_state(&*wire, edit, ChangeCause::Other);
            }
        });
        let outcome = {
            let _entered = self.queue.enter();
            store.request_lock(grant, LockTiming::Async)
        };
        match outcome {
            Ok(LockOutcome::Granted) => CompositionEnd::Committed,
            Ok(LockOutcome::Deferred) | Err(_) => CompositionEnd::Abandoned,
        }
    }

    /// Read the focused store under a synchronous lock: `None` when no field
    /// has focus or the store cannot grant one now.
    fn read<R: 'static>(&self, body: impl FnOnce(&dyn TextStoreRead) -> R + 'static) -> Option<R> {
        let store = self.focused()?;
        let answer: Rc<RefCell<Option<R>>> = Rc::default();
        let out = Rc::clone(&answer);
        let grant = LockGrant::read(move |session| {
            *out.borrow_mut() = Some(body(session));
        });
        {
            let _entered = self.queue.enter();
            let _ = store.request_lock(grant, LockTiming::Sync);
        }
        self.drain();
        answer.take()
    }

    /// Edit the focused store under an asynchronous lock: now when it is
    /// free, else when its frame transaction lets it.
    fn edit(&self, body: impl FnOnce(&mut dyn TextStoreEdit) + 'static) {
        let Some(store) = self.focused() else {
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
}

impl TextStoreHost for TextInputClient {
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
    client: Weak<TextInputClient>,
}

impl TextStoreObserver for ClientObserver {
    fn text_changed(&self, _change: TextChange) {
        if let Some(client) = self.client.upgrade() {
            client.resend_state();
        }
    }

    fn selection_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.resend_state();
        }
    }

    fn layout_changed(&self) {
        if let Some(client) = self.client.upgrade() {
            client.resend_caret();
        }
    }

    fn status_changed(&self) {
        if let Some(client) = self.client.upgrade()
            && client.enabled.get()
            && let (Some(wire), Some(store)) = (client.wire(), client.focused())
        {
            wire.set_content_type(ContentType::of_store(&*store));
            wire.commit();
        }
    }
}
