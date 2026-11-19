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
use flui_foundation::panic::FirstPanic;
use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
use flui_platform::text_store::{
    Composition, CompositionEnd, LockGrant, LockTiming, OwnerCalls, PointMode, Selection,
    SelectionUi, TextChange, TextStore, TextStoreChange, TextStoreEdit, TextStoreError,
    TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreRead, TextStoreStatus,
};
#[cfg(any(target_os = "ios", test))]
use flui_platform::text_store::{EditCommand, PressTarget};

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

    /// The field's caret or composition moved, or the field did: the
    /// candidate window asks for its rectangle again
    /// (`invalidateCharacterCoordinates`). Called on every caret move, typing
    /// included.
    fn invalidate_character_coordinates(&self);

    /// The field itself moved in the window (a scroll, a transform, the
    /// keyboard lifting the view), or the input method serves another field:
    /// what the platform draws over the field from its geometry is asked for
    /// again. Never called for a caret move inside an unmoved field, nor
    /// while the input method holds marked text or the platform's own
    /// selection gesture is under way (`TextClient::set_interacting`): a
    /// move then is told once, when that ends. AppKit draws nothing over the
    /// field, so the default does nothing.
    fn geometry_changed(&self) {}

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
    fn document_changed(&self, change: DocumentChange, marked_text: MarkedText);

    /// The application is about to publish this change. Synchronous reads
    /// still see the previous published document during this notice.
    fn document_will_change(&self, change: DocumentChange);

    /// The focus moved off the field the input method served, to another
    /// field or to none: a menu the platform showed over the old field (UIKit's
    /// edit menu) is dismissed, and nothing else of the input method's state
    /// is torn down. Called once per move, after [`Self::field_changed`] told
    /// the new field. AppKit shows nothing over the field, so the default does
    /// nothing.
    fn field_left(&self) {}
}

/// What the application changed of the focused field's document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentChange {
    /// Its text.
    Text,
    /// Its selection alone.
    Selection,
    /// The composition changed without a text or selection edit.
    Composition,
}

/// Whether this application change ended the input method's marked text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkedText {
    Preserve,
    Discard,
}

/// The notification recipient for the publication currently on the stack.
#[derive(Default)]
enum PublicationContext {
    #[default]
    Idle,
    Active(Option<Weak<dyn InputContext>>),
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
    publication: RefCell<PublicationContext>,
    /// Whether the input method holds marked text it set through this
    /// client (`setMarkedText:` with text, until a commit, an unmark or a
    /// discard).
    marked: Cell<bool>,
    /// The requests made while the client is inside a call into a store.
    queue: HostQueue<MirrorHostOp>,
    retired: Cell<bool>,
    /// Who draws the selection UI of the fields this client serves: UIKit's
    /// text interaction on iOS, the field on macOS.
    selection_ui: SelectionUi,
    /// The field UIKit last asked about an edit command for
    /// (`canPerformAction:withSender:`): a command reaches it only while it
    /// still has focus, so an edit menu shown over one field never edits the
    /// field focus moved to. Held strongly, so the identity it is compared by
    /// cannot be a later store at a freed address; let go on every focus move.
    #[cfg(any(target_os = "ios", test))]
    offered: RefCell<Option<Rc<dyn TextStore>>>,
    /// Whether the platform's own selection gesture is under way over the
    /// field (UIKit's text interaction, from `interactionWillBegin:` to
    /// `interactionDidEnd:`).
    interacting: Cell<bool>,
    /// The field moved while the input method held marked text or a gesture
    /// was under way: [`InputContext::geometry_changed`] is owed, once, when
    /// that ends.
    geometry_owed: Cell<bool>,
    /// The focused field refused a geometry read for lack of layout: what
    /// the platform drew from that read is asked for again
    /// ([`InputContext::geometry_changed`]) once the field reports its layout.
    layout_refused: Cell<bool>,
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

/// Probe at most two UTF-16 units, keeping a surrogate pair whole. The
/// store validates offsets before refusing a protected field's text, so
/// protection never requires copying the password to answer this query.
#[cfg(any(target_os = "ios", test))]
fn scalar_range(
    session: &dyn TextStoreRead,
    at: Utf16Offset,
    backward: bool,
) -> Option<Utf16Range> {
    use flui_platform::text_store::utf16::OffsetError;

    let valid = |range: Utf16Range| -> Result<(), TextStoreError> {
        match session.text(range) {
            Ok(_) | Err(TextStoreError::Protected) => Ok(()),
            Err(error) => Err(error),
        }
    };
    let len = session.document_len();
    let mut at = at.min(len);
    match valid(Utf16Range::collapsed(at)) {
        Ok(()) => {}
        Err(TextStoreError::Offset(OffsetError::SplitsSurrogatePair(_))) => {
            at = Utf16Offset::new(at.get().saturating_sub(1));
        }
        Err(_) => return None,
    }
    if (backward && at == Utf16Offset::ZERO) || (!backward && at == len) {
        return Some(Utf16Range::collapsed(at));
    }
    let start = if backward {
        at.get().saturating_sub(1)
    } else {
        at.get()
    };
    let end = if backward {
        at.get()
    } else {
        at.get().saturating_add(1).min(len.get())
    };
    let range = |start, end| {
        Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end))
            .expect("BUG: bounded scalar probes are ordered")
    };
    let adjacent = range(start, end);
    match valid(adjacent) {
        Ok(()) => Some(adjacent),
        Err(TextStoreError::Offset(OffsetError::SplitsSurrogatePair(_))) => {
            let whole = if backward {
                range(start.saturating_sub(1), end)
            } else {
                range(start, end.saturating_add(1).min(len.get()))
            };
            valid(whole).ok().map(|()| whole)
        }
        Err(_) => None,
    }
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
    /// AppKit's client: the field draws its own selection UI. Also the
    /// conformance table's (`feature = "testing"`).
    #[cfg(any(target_os = "macos", test, feature = "testing"))]
    pub(crate) fn new() -> Rc<Self> {
        Self::with_selection_ui(SelectionUi::Field)
    }

    /// UIKit's client: the responder's `UITextInteraction` draws the served
    /// field's selection UI, and its edit menu reaches the field as edit
    /// commands.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn for_uikit() -> Rc<Self> {
        Self::with_selection_ui(SelectionUi::Platform)
    }

    fn with_selection_ui(selection_ui: SelectionUi) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            focused: RefCell::new(None),
            context: RefCell::new(None),
            publication: RefCell::new(PublicationContext::Idle),
            marked: Cell::new(false),
            queue: HostQueue::new(),
            retired: Cell::new(false),
            selection_ui,
            #[cfg(any(target_os = "ios", test))]
            offered: RefCell::new(None),
            interacting: Cell::new(false),
            geometry_owed: Cell::new(false),
            layout_refused: Cell::new(false),
            this: this.clone(),
        })
    }

    /// The input method's context for this view, once the view exists.
    pub(crate) fn bind(&self, context: Rc<dyn InputContext>) {
        if !self.retired.get() {
            let previous = self.context.replace(Some(context));
            let mut failure = FirstPanic::new();
            if let Some(previous) = previous {
                failure.contain(|| {
                    let _entered = self.queue.enter();
                    previous.field_changed(None);
                });
                failure.contain(|| drop(previous));
            }
            // A field may have taken focus before the view existed.
            failure.contain(|| self.sync_field());
            self.drain_after(&mut failure);
            failure.resume();
        }
    }

    /// The view is going away: the focused field is let go and the input method's
    /// context is never reached again. A later call is a no-op.
    pub(crate) fn retire(&self) {
        if self.retired.replace(true) {
            return;
        }
        // Withdraw authority before callbacks: even an interrupted keyboard
        // notice cannot leave a retired client serving its previous field.
        let context = self.context.borrow_mut().take();
        let store = self.focused.borrow_mut().take();
        #[cfg(any(target_os = "ios", test))]
        let offered = self.offered.borrow_mut().take();
        self.marked.set(false);
        self.forget_field_geometry();
        let mut failure = FirstPanic::new();
        // The field an edit menu was offered over is let go too: a retired
        // client keeps no store alive.
        #[cfg(any(target_os = "ios", test))]
        failure.contain(|| drop(offered));
        if let Some(context) = context {
            failure.contain(|| {
                let _entered = self.queue.enter();
                context.field_changed(None);
            });
            failure.contain(|| drop(context));
        }
        if let Some(store) = store {
            failure.contain(|| store.set_observer(None));
            failure.contain(|| drop(store));
        }
        // Accepted completions still commit in place. When retirement is
        // reentrant, the outer callback drains them after releasing its guard.
        self.drain_after(&mut failure);
        failure.resume();
    }

    /// The store the client serves now.
    fn focused(&self) -> Option<Rc<dyn TextStore>> {
        self.focused.borrow().clone()
    }

    /// AppKit's context, held for no borrow: its calls may come back in.
    fn context(&self) -> Option<Rc<dyn InputContext>> {
        self.context.borrow().clone()
    }

    /// A publication never retargets a later notice to a replacement context.
    fn document_context(&self) -> Option<Rc<dyn InputContext>> {
        let current = self.context()?;
        let recipient = match &*self.publication.borrow() {
            PublicationContext::Idle => return Some(current),
            PublicationContext::Active(recipient) => recipient.as_ref()?.upgrade()?,
        };
        Rc::ptr_eq(&current, &recipient).then_some(recipient)
    }

    fn publish_change(
        &self,
        change: TextStoreChange,
        publish: &mut dyn FnMut(&mut OwnerCalls),
        calls: &mut OwnerCalls,
    ) {
        let context = self.context();
        let previous = self.publication.replace(PublicationContext::Active(
            context.as_ref().map(Rc::downgrade),
        ));
        let entered = self.queue.enter();
        let notices: &[DocumentChange] = match change {
            TextStoreChange::Text(_) => &[DocumentChange::Text, DocumentChange::Selection],
            TextStoreChange::Selection => &[DocumentChange::Selection],
            TextStoreChange::Composition => {
                &[DocumentChange::Composition, DocumentChange::Selection]
            }
            _ => &[],
        };
        for &change in notices {
            if let Some(recipient) = self.document_context() {
                calls.run(|| recipient.document_will_change(change));
                calls.run(|| drop(recipient));
            }
        }
        // Keep the caller's ledger through every Did callback. Catch only to
        // restore this stack frame if publication itself unexpectedly unwinds.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a publication panic still restores this frame's call ledger"
        )]
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| publish(calls)))
        {
            calls.keep(payload);
        }
        let retired = self.publication.replace(previous);
        drop(retired);
        drop(entered);
        calls.run(|| drop(context));
        while let Some(op) = self.queue.next() {
            calls.run(|| self.run(op));
        }
    }

    /// A keyboard callback may queue work, replace its context or retire the
    /// client. Finish that work before resuming its first failure, and fence
    /// the old context after a reentrant loss of authority.
    fn notify_context(
        &self,
        context: Rc<dyn InputContext>,
        notify: impl FnOnce(&dyn InputContext),
    ) {
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            let _entered = self.queue.enter();
            notify(&*context);
        });
        let current = self.context();
        if !current
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &context))
        {
            failure.contain(|| {
                let _entered = self.queue.enter();
                context.field_changed(None);
            });
        }
        failure.contain(|| drop(current));
        failure.contain(|| drop(context));
        self.drain_after(&mut failure);
        failure.resume();
    }

    /// Tell AppKit's context to forget its marked text, if it holds any, as
    /// a call into the focused store: what the input method does in answer
    /// (a commit, an unmark) edits the store under this call, and a host
    /// operation it causes waits for the call to return.
    fn discard_marked_text(&self) {
        if !self.marked.replace(false) {
            return;
        }
        if let Some(context) = self.document_context() {
            self.notify_context(context, |context| context.discard_marked_text());
        }
        self.settle_owed_geometry();
    }

    fn invalidate_character_coordinates(&self) {
        if let Some(context) = self.context() {
            self.notify_context(context, |context| {
                context.invalidate_character_coordinates();
            });
        }
    }

    /// The focused field moved in the window, or the client serves another
    /// field: the input method asks for its geometry again
    /// ([`InputContext::geometry_changed`]), now, or once the marked text or
    /// the platform's gesture under way ends.
    fn field_moved(&self) {
        if self.focused.borrow().is_none() {
            // No field is served: nothing it drew is out of date.
            self.geometry_owed.set(false);
            return;
        }
        if self.marked.get() || self.interacting.get() {
            self.geometry_owed.set(true);
            return;
        }
        self.geometry_owed.set(false);
        if let Some(context) = self.context() {
            self.notify_context(context, |context| context.geometry_changed());
        }
    }

    /// A move owed while marked text or a gesture lasted is told now that
    /// neither does.
    fn settle_owed_geometry(&self) {
        if self.geometry_owed.get() && !self.marked.get() && !self.interacting.get() {
            self.field_moved();
        }
    }

    /// The focused field reported a layout change: a geometry read it
    /// refused for lack of layout is owed as a move of the field, told under
    /// the same holds ([`Self::field_moved`]).
    fn layout_settled(&self) {
        if self.layout_refused.replace(false) {
            self.field_moved();
        }
    }

    /// Forget what is owed of the served field's geometry, and the platform
    /// gesture that was under way over it: the field is no longer served, or
    /// another is (whose own geometry the caller asks for). A gesture whose
    /// end UIKit never reports, focus having left mid-gesture, does not hold
    /// the next field's geometry back.
    fn forget_field_geometry(&self) {
        self.interacting.set(false);
        self.geometry_owed.set(false);
        self.layout_refused.set(false);
    }

    /// The platform's own selection gesture over the field began (`true`,
    /// UIKit's `interactionWillBegin:`) or ended (`false`,
    /// `interactionDidEnd:`, or the interaction taken off its view): a move
    /// of the field during it is told once, at its end.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn set_interacting(&self, interacting: bool) {
        self.interacting.set(interacting);
        if !interacting {
            self.settle_owed_geometry();
        }
    }

    /// Tell the input method which field it serves, by its status: what it
    /// does with it (a secure keyboard, secure event input, the keyboard's
    /// traits) follows the field. Called wherever the answer can change:
    /// focus, the focused field's status, the window's key status, binding
    /// and retirement.
    fn sync_field(&self) {
        let store = if self.retired.get() {
            None
        } else {
            self.focused()
        };
        let mut failure = FirstPanic::new();
        let status = failure.contain(|| {
            // The store's `status` is application code, which may move
            // focus: a request it makes waits in the queue, and the refocus
            // the drain below runs syncs again with the focus it gives.
            let _entered = self.queue.enter();
            store.as_ref().map(|store| store.status())
        });
        // The input method's answer may come back into the client: no
        // borrow is held.
        if let Some(status) = status
            && let Some(context) = self.context()
        {
            failure.contain(|| {
                self.notify_context(context, |context| context.field_changed(status));
            });
        }
        self.drain_after(&mut failure);
        failure.contain(|| drop(store));
        failure.resume();
    }

    /// The view's window became or stopped being the key window: the keys
    /// typed reach this window's field only while it is.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn key_window_changed(&self) {
        self.sync_field();
    }

    /// Apply the requests queued while the client was inside a store call.
    fn drain(&self) {
        let mut failure = FirstPanic::new();
        self.drain_after(&mut failure);
        failure.resume();
    }

    fn drain_after(&self, failure: &mut FirstPanic) {
        while let Some(op) = self.queue.next() {
            failure.contain(|| self.run(op));
        }
    }

    fn run(&self, op: MirrorHostOp) {
        match op {
            MirrorHostOp::Focus(store) => self.refocus(store),
            MirrorHostOp::Complete(store) => {
                let _entered = self.queue.enter();
                let mut failure = FirstPanic::new();
                // Whoever was told `Deferred` owes nothing more: what AppKit
                // does not commit is committed in place here.
                let focused = self
                    .focused()
                    .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                let end = if focused {
                    failure
                        .contain(|| self.end_composition(&store))
                        .unwrap_or(CompositionEnd::Abandoned)
                } else {
                    CompositionEnd::Abandoned
                };
                if end != CompositionEnd::Committed {
                    failure.contain(|| {
                        crate::text::refusal::commit_in_place(&*store);
                    });
                }
                failure.contain(|| drop(store));
                failure.resume();
            }
        }
    }

    fn refocus(&self, store: Option<Rc<dyn TextStore>>) {
        // A context reached below may itself request focus. Commit this
        // transition before applying that newer request.
        let _entered = self.queue.enter();
        let same = match (&store, &self.focused()) {
            (Some(next), Some(current)) => Rc::ptr_eq(next, current),
            (None, None) => true,
            _ => false,
        };
        if same || self.retired.get() {
            return;
        }
        // What is owed of the field losing focus is not paid to it: ending
        // its composition below settles nothing for it, and the next field's
        // geometry is asked for once it is served.
        self.forget_field_geometry();
        // An edit command offered for the field losing focus is not the
        // next field's.
        #[cfg(any(target_os = "ios", test))]
        let offered = self.offered.borrow_mut().take();
        #[cfg(any(target_os = "ios", test))]
        contain_owner_callback(|| drop(offered));
        let left = self.focused();
        if let Some(previous) = &left {
            // AppKit's composition belongs to the field losing focus: what
            // the input method makes of the discard reaches that field, and
            // what it leaves composing there is committed in place, so no
            // field keeps a composing range no input method owns.
            self.discard_marked_text();
            crate::text::refusal::commit_in_place(&**previous);
            *self.focused.borrow_mut() = None;
            contain_owner_callback(|| previous.set_observer(None));
        }
        if let Some(store) = store {
            let observer: Rc<dyn TextStoreObserver> = Rc::new(ClientObserver {
                client: self.this.clone(),
                store: Rc::downgrade(&store),
            });
            *self.focused.borrow_mut() = Some(Rc::clone(&store));
            contain_owner_callback(|| store.set_observer(Some(observer)));
        }
        self.sync_field();
        self.invalidate_character_coordinates();
        // Another field: what the platform drew over the last one is asked
        // for again.
        let serves = self.focused.borrow().is_some();
        if serves {
            self.field_moved();
        }
        if left.is_some()
            && let Some(context) = self.context()
        {
            self.notify_context(context, |context| context.field_left());
        }
        contain_owner_callback(|| drop(left));
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
        let mut failure = FirstPanic::new();
        let answer = failure
            .contain(|| {
                let _entered = self.queue.enter();
                read(&*store, body)
            })
            .flatten();
        self.drain_after(&mut failure);
        failure.contain(|| drop(store));
        failure.resume();
        answer
    }

    /// Read the focused field's geometry ([`Self::read`]): a read it refuses
    /// for lack of layout makes its layout notice ask the platform for the
    /// geometry again ([`Self::layout_settled`]), since the platform asks
    /// only when told. `None` for any refusal.
    fn read_geometry<R: 'static>(
        &self,
        body: impl FnOnce(&dyn TextStoreRead) -> Result<R, TextStoreError> + 'static,
    ) -> Option<R> {
        match self.read(body)? {
            Ok(answer) => Some(answer),
            Err(error) => {
                if error == TextStoreError::NoLayout {
                    self.layout_refused.set(true);
                }
                tracing::debug!(?error, "the focused field refused a geometry read");
                None
            }
        }
    }

    /// Edit the focused store under an asynchronous lock: now when it is
    /// free, else when its frame transaction lets it.
    fn edit(&self, body: impl FnOnce(&mut dyn TextStoreEdit) + 'static) {
        let Some(store) = self.focused() else {
            tracing::trace!("an input method edit with no focused field; dropped");
            return;
        };
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            let _entered = self.queue.enter();
            if let Err(error) = store.request_lock(LockGrant::read_write(body), LockTiming::Async) {
                tracing::debug!(?error, "the focused store refused the input method's edit");
            }
        });
        self.drain_after(&mut failure);
        failure.contain(|| drop(store));
        failure.resume();
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

    /// Whether the input method holds marked text it set through this client:
    /// a change of the field's traits waits until it ends rather than reload
    /// the keyboard under it.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn composes(&self) -> bool {
        self.marked.get()
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
        self.settle_owed_geometry();
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
        self.settle_owed_geometry();
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
        self.settle_owed_geometry();
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
        self.read_geometry(move |session| {
            let range = query_target(session, range);
            session
                .rect_for_range(range)
                .map(|rect| (rect.bounds, range))
        })
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

    /// The scalar before or after a position in the focused field.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn extended_range(&self, at: Utf16Offset, backward: bool) -> Option<Utf16Range> {
        self.read(move |session| scalar_range(session, at, backward))
            .flatten()
    }

    /// `selectionRectsForRange:` — one rectangle per line `range` crosses,
    /// clamped to the document, in the window's logical coordinates; none
    /// when no field has focus or its layout has none.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn selection_rects(
        &self,
        range: Utf16Range,
    ) -> Vec<super::selection_rects::SelectionRect> {
        self.read_geometry(move |session| {
            super::selection_rects::selection_rects(session, clamped(range, session.document_len()))
        })
        .unwrap_or_default()
    }

    /// `canPerformAction:withSender:` for an edit-menu item: whether the
    /// focused field would perform `command` now; `false` without one. The
    /// field asked about is the one a command may reach afterwards
    /// ([`Self::perform_edit_command`]).
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn edit_command_enabled(&self, command: EditCommand) -> bool {
        let offered = self.focused();
        let replaced = self.offered.replace(offered);
        contain_owner_callback(|| drop(replaced));
        self.call_focused(|store| store.edit_command_enabled(command))
            .unwrap_or(false)
    }

    /// `cut:`, `copy:`, `paste:`, `select:` and `selectAll:` from the edit
    /// menu: the focused field performs `command` as its own menu would, as
    /// a call into its store, so a host operation its code asks for (a paste
    /// ending the composition, a focus move) waits for it to return. Refused
    /// unless the focused field is the one UIKit last asked about
    /// ([`Self::edit_command_enabled`]): a menu offered over one field never
    /// edits the field focus moved to since. Whether the field took it.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn perform_edit_command(&self, command: EditCommand) -> bool {
        let offered_here = match (&*self.offered.borrow(), &*self.focused.borrow()) {
            (Some(offered), Some(focused)) => Rc::ptr_eq(offered, focused),
            _ => false,
        };
        if !offered_here {
            tracing::debug!(
                ?command,
                "an edit command for a field that lost focus; refused"
            );
            return false;
        }
        self.call_focused(|store| store.perform_edit_command(command))
            .unwrap_or(false)
    }

    /// Run `call` on the focused store as a call into it, outside any lock
    /// and with no borrow held; `None` when no field has focus or the call
    /// panicked, whose failure is resumed once the requests it queued have
    /// run.
    #[cfg(any(target_os = "ios", test))]
    fn call_focused<R>(&self, call: impl FnOnce(&dyn TextStore) -> R) -> Option<R> {
        let store = self.focused()?;
        let mut failure = FirstPanic::new();
        let answer = failure.contain(|| {
            let _entered = self.queue.enter();
            call(&*store)
        });
        self.drain_after(&mut failure);
        failure.contain(|| drop(store));
        failure.resume();
        answer
    }

    /// `caretRectForPosition:` — the caret at `at`, held inside the part of
    /// the field the window shows ([`super::selection_rects::caret_in`]);
    /// `None` when no field has focus, it shows nothing or its layout has
    /// none.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn caret_rect(&self, at: Utf16Offset) -> Option<Bounds<f64>> {
        self.read_geometry(move |session| super::selection_rects::caret_in(session, at))
            .flatten()
    }

    /// What a press at `point`, in the window's logical coordinates, lands
    /// on ([`TextStore::press_target_at`]), asked of the focused store as a
    /// call into it, outside any lock: answering runs code the store does not
    /// own (hit tests, recognizers' admission predicates). `None` without a
    /// field, when the store refuses (no layout, its window unavailable) or
    /// when the call panics: the platform begins no gesture then.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn press_target_at(&self, point: Point<f64>) -> Option<PressTarget> {
        self.call_focused(|store| store.press_target_at(point))
            .and_then(Result::ok)
    }

    /// The scalar under a point in the focused field.
    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn character_range_at_point(&self, point: Point<f64>) -> Option<Utf16Range> {
        self.read(move |session| {
            let at = session.index_at_point(point, PointMode::EXACT).ok()?;
            scalar_range(session, at, false)
        })
        .flatten()
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
        let mut marked_text = MarkedText::Preserve;
        if self.marked.get() {
            let composing = self.read(|session| session.composition().is_some());
            if composing == Some(false) {
                self.marked.set(false);
                marked_text = MarkedText::Discard;
            }
        }
        if let Some(context) = self.document_context() {
            // As a call into the focused store: what the input method does
            // in answer edits the store under it, and a host operation it
            // causes waits for it to return.
            self.notify_context(context, |context| {
                context.document_changed(change, marked_text);
            });
        }
        self.drain();
        self.settle_owed_geometry();
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
            let mut failure = FirstPanic::new();
            failure.contain(|| self.run(op));
            self.drain_after(&mut failure);
            failure.resume();
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
        let mut failure = FirstPanic::new();
        let end = failure
            .contain(|| {
                let _entered = self.queue.enter();
                self.end_composition(store)
            })
            .unwrap_or(CompositionEnd::Abandoned);
        self.drain_after(&mut failure);
        failure.resume();
        Ok(end)
    }

    /// macOS has no soft keyboard, and UIKit keeps the keyboard of the
    /// first responder, which the focused field's responder stays while it
    /// has the input.
    fn show_keyboard(&self, _: &Rc<dyn TextStore>) {}

    fn selection_ui(&self) -> SelectionUi {
        self.selection_ui
    }
}

/// What the focused store tells the client when the application changes the
/// field.
struct ClientObserver {
    client: Weak<TextClient>,
    /// The store it was installed on, which owns it: compared by address
    /// with the store the client serves, and never upgraded.
    store: Weak<dyn TextStore>,
}

impl TextStoreObserver for ClientObserver {
    fn publish_change(
        &self,
        change: TextStoreChange,
        publish: &mut dyn FnMut(&mut OwnerCalls),
        calls: &mut OwnerCalls,
    ) {
        if let Some(client) = self.serving() {
            client.publish_change(change, publish, calls);
            calls.run(|| drop(client));
        } else {
            publish(calls);
        }
    }

    fn text_changed(&self, _change: TextChange) {
        if let Some(client) = self.serving() {
            client.application_edited(DocumentChange::Text);
        }
    }

    fn selection_changed(&self) {
        if let Some(client) = self.serving() {
            client.application_edited(DocumentChange::Selection);
        }
    }

    fn composition_changed(&self) {
        if let Some(client) = self.serving() {
            client.application_edited(DocumentChange::Composition);
        }
    }

    /// The focused field's caret, composition or layout moved, typing
    /// included: the candidate window asks for its rectangle again
    /// (`invalidateCharacterCoordinates` on AppKit; UIKit asks
    /// `firstRectForRange:` itself and is told nothing). A geometry read the
    /// field refused for lack of layout is now owed as a move of the field
    /// ([`InputContext::geometry_changed`], under the same holds).
    fn layout_changed(&self) {
        if let Some(client) = self.serving() {
            client.invalidate_character_coordinates();
            client.layout_settled();
        }
    }

    /// The focused field itself moved in the window: what the platform
    /// draws over it from its geometry is asked for again
    /// ([`InputContext::geometry_changed`]), not during marked text or a
    /// gesture under way.
    fn field_moved(&self) {
        if let Some(client) = self.serving() {
            client.field_moved();
        }
    }

    fn status_changed(&self) {
        if let Some(client) = self.serving() {
            client.sync_field();
        }
    }
}

impl ClientObserver {
    /// The client, while the store this observer was installed on is the
    /// one it serves: a store focus moved off that still reports (a
    /// publication under way when it lost focus) reaches no input method.
    fn serving(&self) -> Option<Rc<TextClient>> {
        let client = self.client.upgrade()?;
        let serves = client
            .focused
            .borrow()
            .as_ref()
            .is_some_and(|focused| std::ptr::addr_eq(Rc::as_ptr(focused), self.store.as_ptr()));
        serves.then_some(client)
    }
}
