//! The Android window's text input: the focused field's `TextStore`
//! (ADR-0090) served to the input method through `FluiView`'s
//! `InputConnection` (ADR-0247).
//!
//! [`AndroidTextInput`] is the window's `TextStoreHost` (ADR-0135). The input
//! method asks synchronously on the Java main thread and never waits for the
//! owner: it reads and edits the view's mirror of the field, and the owner
//! keeps that mirror honest. Focusing a field publishes a snapshot of its
//! store, which becomes the mirror, and shows the soft keyboard. Each edit the
//! view reports is replayed into the store under an asynchronous lock, in
//! order, now if the store is free and else when its frame transaction lets
//! it; an edit made on a snapshot older than the latest one the owner
//! published is dropped (`shared::android_ime`). When the field changes
//! outside the input method's edits (the application's own edit, a moved
//! caret) the store's observer publishes a new snapshot: a text change
//! restarts the input method, a selection change only tells it.
//!
//! A request the presentation makes from inside a store call the host started
//! waits and runs, in order, when that call returns.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_platform::text_store::{
    CompositionEnd, LockGrant, LockOutcome, LockTiming, TextChange, TextStore, TextStoreEdit,
    TextStoreHost, TextStoreHostError, TextStoreObserver, TextStoreRead,
};
use jni::{JValue, jni_sig, jni_str};

use crate::shared::android_ime::{
    CURSOR_UPDATE_IMMEDIATE, CURSOR_UPDATE_MONITOR, ImeEdit, Snapshot, cursor_anchor, editor_info,
    replay_current,
};
use crate::shared::os_version::Since;
use crate::shared::os_version::android::{AndroidApi, StylusHandwriting};
use crate::shared::panic_boundary::contain_owner_callback;

use super::java_view::JavaView;

/// A host request made while the host was already inside a call into a
/// store: applied, in order, when that call returns.
enum HostOp {
    Focus(Option<Rc<dyn TextStore>>),
    Complete(Rc<dyn TextStore>),
}

/// How a published snapshot reaches the view.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Publish {
    /// A field took focus: the view becomes its editor.
    Focus,
    /// The text changed: the input method restarts on the new text.
    Restart,
    /// Only the selection or the composition moved.
    Selection,
}

/// The window's text input (see the module doc).
pub struct AndroidTextInput {
    view: Arc<JavaView>,
    /// Whether the input method can start stylus handwriting on the view.
    handwriting: Option<Since<StylusHandwriting>>,
    focused: RefCell<Option<Rc<dyn TextStore>>>,
    /// The generation of the snapshot published last; every replayed edit
    /// must have been made on it.
    published: Rc<Cell<u32>>,
    /// What `requestCursorUpdates` last asked for.
    cursor_mode: Cell<i32>,
    entry: Cell<u32>,
    pending: RefCell<VecDeque<HostOp>>,
    retired: Cell<bool>,
    this: Weak<Self>,
}

impl std::fmt::Debug for AndroidTextInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidTextInput").finish_non_exhaustive()
    }
}

/// Counts one call into a store, however it ends.
struct Entered<'a>(&'a Cell<u32>);

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// Publish `snapshot` to the view: it becomes the mirror the next reports are
/// diffed against, and the view takes it as `how` says.
fn publish(view: &Arc<JavaView>, snapshot: Snapshot, how: Publish, status: (i32, i32, bool)) {
    view.install_mirror(&snapshot);
    view.set_composing(snapshot.composition.is_some());
    let [
        selection_start,
        selection_end,
        composition_start,
        composition_end,
    ] = snapshot.java_offsets();
    // The generation wraps; Java carries its bits as an opaque `int`.
    let generation = snapshot.generation.cast_signed();
    let text = snapshot.text;
    view.post(move |env, view| {
        let text = env.new_string(&text)?;
        match how {
            Publish::Focus => {
                let (input_type, options, stylus) = status;
                env.call_method(
                    view,
                    jni_str!("focusEditor"),
                    jni_sig!("(IIZLjava/lang/String;IIIII)V"),
                    &[
                        JValue::Int(input_type),
                        JValue::Int(options),
                        JValue::Bool(stylus),
                        JValue::Object(&text),
                        JValue::Int(selection_start),
                        JValue::Int(selection_end),
                        JValue::Int(composition_start),
                        JValue::Int(composition_end),
                        JValue::Int(generation),
                    ],
                )?;
            }
            Publish::Restart | Publish::Selection => {
                env.call_method(
                    view,
                    jni_str!("pushState"),
                    jni_sig!("(Ljava/lang/String;IIIIIZ)V"),
                    &[
                        JValue::Object(&text),
                        JValue::Int(selection_start),
                        JValue::Int(selection_end),
                        JValue::Int(composition_start),
                        JValue::Int(composition_end),
                        JValue::Int(generation),
                        JValue::Bool(how == Publish::Restart),
                    ],
                )?;
            }
        }
        Ok(())
    });
}

impl AndroidTextInput {
    pub(super) fn new(view: Arc<JavaView>, api: AndroidApi) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            view,
            handwriting: api.since::<StylusHandwriting>(),
            focused: RefCell::new(None),
            published: Rc::new(Cell::new(0)),
            cursor_mode: Cell::new(0),
            entry: Cell::new(0),
            pending: RefCell::new(VecDeque::new()),
            retired: Cell::new(false),
            this: this.clone(),
        })
    }

    /// The window closed: detach the focused field, if any, for good.
    pub(super) fn retire(&self) {
        self.retired.set(true);
        self.pending.borrow_mut().clear();
        self.detach();
    }

    fn focused(&self) -> Option<Rc<dyn TextStore>> {
        self.focused.borrow().clone()
    }

    fn enter(&self) -> Entered<'_> {
        self.entry.set(self.entry.get() + 1);
        Entered(&self.entry)
    }

    fn next_generation(published: &Cell<u32>) -> u32 {
        let next = published.get().wrapping_add(1);
        published.set(next);
        next
    }

    fn status(&self, store: &dyn TextStore) -> (i32, i32, bool) {
        let (input_type, options) = editor_info(store.status());
        let stylus = self.handwriting.is_some();
        (input_type, options, stylus)
    }

    /// Run the requests queued while the host was inside a store call.
    fn drain(&self) {
        if self.entry.get() > 0 {
            return;
        }
        loop {
            let next = self.pending.borrow_mut().pop_front();
            match next {
                Some(HostOp::Focus(store)) => self.refocus(store),
                Some(HostOp::Complete(store)) => {
                    let focused = self
                        .focused()
                        .is_some_and(|focused| Rc::ptr_eq(&focused, &store));
                    if !focused || self.end_composition(&store) == CompositionEnd::Abandoned {
                        let _ = flui_platform::text_store::commit_composition_in_place(&*store);
                    }
                }
                None => return,
            }
        }
    }

    /// One input method edit the view reported: replayed into the focused
    /// store in order, or dropped when it was made on an older snapshot.
    pub(super) fn apply(&self, edit: ImeEdit) {
        let Some(store) = self.focused() else {
            return;
        };
        let published = Rc::clone(&self.published);
        let view = Arc::clone(&self.view);
        let status = self.status(&*store);
        let this = self.this.clone();
        let edited = Rc::clone(&store);
        let grant = LockGrant::read_write(move |session| {
            let outcome = replay_current(&edit, published.get(), session);
            // Focus may have moved to another field while the grant waited: its
            // snapshot is the one the view mirrors now, and a restart on this
            // store would overwrite it.
            let still_focused = this
                .upgrade()
                .and_then(|input| input.focused())
                .is_some_and(|current| Rc::ptr_eq(&current, &edited));
            if outcome.restarts_the_view() && still_focused {
                // The store refused a step, or the edit was dropped as stale:
                // the input method's text no longer matches the store's.
                let generation = Self::next_generation(&published);
                publish(
                    &view,
                    Snapshot::read(&*session, generation),
                    Publish::Restart,
                    status,
                );
            }
        });
        {
            let _entered = self.enter();
            if let Err(error) = store.request_lock(grant, LockTiming::Async) {
                tracing::debug!(?error, "the focused store refused the input method's edit");
                self.publish_now(Publish::Restart);
            }
        }
        self.drain();
    }

    /// Publish the focused store now, or when its frame transaction lets it.
    fn publish_now(&self, how: Publish) {
        let Some(store) = self.focused() else {
            return;
        };
        let published = Rc::clone(&self.published);
        let view = Arc::clone(&self.view);
        let status = self.status(&*store);
        let grant = move |session: &dyn TextStoreRead| {
            let generation = Self::next_generation(&published);
            publish(&view, Snapshot::read(session, generation), how, status);
        };
        let _entered = self.enter();
        let sync = {
            let grant = grant.clone();
            store.request_lock(LockGrant::read(grant), LockTiming::Sync)
        };
        if sync.is_err() {
            let _ = store.request_lock(LockGrant::read(grant), LockTiming::Async);
        }
    }

    fn detach(&self) {
        let previous = self.focused.borrow_mut().take();
        let Some(store) = previous else {
            return;
        };
        self.view.set_composing(false);
        self.cursor_mode.set(0);
        contain_owner_callback(|| store.set_observer(None));
        self.view.post(|env, view| {
            env.call_method(view, jni_str!("blurEditor"), jni_sig!("()V"), &[])
                .map(drop)
        });
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
        self.detach();
        let Some(store) = store else {
            return;
        };
        *self.focused.borrow_mut() = Some(Rc::clone(&store));
        let observer: Rc<dyn TextStoreObserver> = Rc::new(MirrorObserver {
            input: self.this.clone(),
        });
        contain_owner_callback(|| store.set_observer(Some(observer)));
        self.publish_now(Publish::Focus);
    }

    /// End the input method's composition in `store`: commit it in the store
    /// and restart the input method on the result, which drops its own.
    fn end_composition(&self, store: &Rc<dyn TextStore>) -> CompositionEnd {
        let published = Rc::clone(&self.published);
        let view = Arc::clone(&self.view);
        let status = self.status(&**store);
        let grant = LockGrant::read_write(move |session: &mut dyn TextStoreEdit| {
            if session.composition().is_some() {
                let _ = session.set_composition(None);
            }
            let generation = Self::next_generation(&published);
            view.set_composing(false);
            publish(
                &view,
                Snapshot::read(&*session, generation),
                Publish::Restart,
                status,
            );
        });
        let outcome = {
            let _entered = self.enter();
            store.request_lock(grant, LockTiming::Async)
        };
        match outcome {
            Ok(LockOutcome::Granted) => CompositionEnd::Committed,
            Ok(LockOutcome::Deferred) => CompositionEnd::Deferred,
            Err(_) => CompositionEnd::Abandoned,
        }
    }

    /// The input method performs its editor action (`performEditorAction`):
    /// the action acts on the text the input method shows, so a composition
    /// still open in the focused store is committed first. An input method
    /// may send the action without finishing its composition (Samsung
    /// Keyboard's Done does); the field would otherwise take the action's
    /// Enter for the input method's own and ignore it. The commit is queued
    /// like an edit, so the key that follows lands after it.
    pub(super) fn editor_action(&self) {
        let Some(store) = self.focused() else {
            return;
        };
        let published = Rc::clone(&self.published);
        let view = Arc::clone(&self.view);
        let status = self.status(&*store);
        let grant = LockGrant::read_write(move |session: &mut dyn TextStoreEdit| {
            if session.composition().is_none() {
                return;
            }
            let _ = session.set_composition(None);
            view.set_composing(false);
            let generation = Self::next_generation(&published);
            publish(
                &view,
                Snapshot::read(&*session, generation),
                Publish::Restart,
                status,
            );
        });
        {
            let _entered = self.enter();
            if let Err(error) = store.request_lock(grant, LockTiming::Async) {
                tracing::debug!(
                    ?error,
                    "the focused store refused the editor action's commit"
                );
            }
        }
        self.drain();
    }

    /// `requestCursorUpdates(mode)`.
    pub(super) fn cursor_updates(&self, mode: i32) {
        self.cursor_mode.set(mode & CURSOR_UPDATE_MONITOR);
        if mode & CURSOR_UPDATE_IMMEDIATE != 0 {
            self.push_cursor();
        }
    }

    /// Tell the input method where the caret and the field are.
    fn push_cursor(&self) {
        let Some(store) = self.focused() else {
            return;
        };
        let found: Rc<Cell<Option<[f32; 7]>>> = Rc::default();
        let out = Rc::clone(&found);
        let scale = self.view.scale();
        let grant = LockGrant::read(move |session| {
            let caret = session.rect_for_range(session.selection().range());
            if let Ok(caret) = caret {
                out.set(cursor_anchor(
                    caret.bounds,
                    session.document_bounds().ok(),
                    scale,
                ));
            }
        });
        {
            let _entered = self.enter();
            if store.request_lock(grant, LockTiming::Sync).is_err() {
                return;
            }
        }
        let Some(anchor) = found.get() else {
            return;
        };
        self.view.post(move |env, view| {
            env.call_method(
                view,
                jni_str!("updateCursorAnchor"),
                jni_sig!("(FFFFFFF)V"),
                &anchor.map(JValue::Float),
            )
            .map(drop)
        });
    }

    /// The focused field's bounds, for a stylus press that may start
    /// handwriting; `None` when no field is focused or laid out.
    pub(super) fn field_bounds(&self) -> Option<Bounds<f64>> {
        let store = self.focused()?;
        let found: Rc<Cell<Option<Bounds<f64>>>> = Rc::default();
        let out = Rc::clone(&found);
        let grant = LockGrant::read(move |session| out.set(session.document_bounds().ok()));
        let _entered = self.enter();
        let _granted = store.request_lock(grant, LockTiming::Sync).ok()?;
        found.get()
    }

    /// Hand the stylus stroke that just began to the input method.
    pub(super) fn start_handwriting(&self, _handwriting: Since<StylusHandwriting>) {
        self.view.post(|env, view| {
            let started = env
                .call_method(view, jni_str!("startHandwriting"), jni_sig!("()Z"), &[])?
                .z()?;
            tracing::info!(target: "flui_native::android", started, "stylus handwriting requested");
            Ok(())
        });
    }
}

impl TextStoreHost for AndroidTextInput {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        if self.entry.get() > 0 {
            self.pending.borrow_mut().push_back(HostOp::Focus(store));
            return;
        }
        self.refocus(store);
        self.drain();
    }

    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        if self.retired.get() || !self.view.is_ready() {
            return Err(TextStoreHostError::Unavailable);
        }
        let queued = self.pending.borrow().iter().rev().find_map(|op| match op {
            HostOp::Focus(store) => Some(store.clone()),
            HostOp::Complete(_) => None,
        });
        let focused = queued.unwrap_or_else(|| self.focused());
        if !focused.is_some_and(|focused| Rc::ptr_eq(&focused, store)) {
            return Err(TextStoreHostError::NotFocused);
        }
        if self.entry.get() > 0 {
            self.pending
                .borrow_mut()
                .push_back(HostOp::Complete(Rc::clone(store)));
            return Ok(CompositionEnd::Deferred);
        }
        let end = self.end_composition(store);
        self.drain();
        Ok(end)
    }
}

impl crate::host::Text for AndroidTextInput {}

/// What the store tells the view when the application changes the field.
struct MirrorObserver {
    input: Weak<AndroidTextInput>,
}

impl TextStoreObserver for MirrorObserver {
    fn text_changed(&self, _change: TextChange) {
        if let Some(input) = self.input.upgrade() {
            input.publish_now(Publish::Restart);
        }
    }

    fn selection_changed(&self) {
        if let Some(input) = self.input.upgrade() {
            input.publish_now(Publish::Selection);
        }
    }

    fn layout_changed(&self) {
        if let Some(input) = self.input.upgrade()
            && input.cursor_mode.get() & CURSOR_UPDATE_MONITOR != 0
        {
            input.push_cursor();
        }
    }

    fn status_changed(&self) {
        if let Some(input) = self.input.upgrade() {
            input.publish_now(Publish::Focus);
        }
    }
}
