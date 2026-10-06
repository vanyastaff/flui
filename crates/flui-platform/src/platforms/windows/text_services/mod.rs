//! The Win32 text services: TSF, the input-method path of a FLUI window
//! (ADR-0090 §3; no IMM32).
//!
//! One [`TextServices`] per window activates the thread's `ITfThreadMgr`
//! and associates an empty document manager with the window, so an input
//! method sees "no editable field" until a field takes focus. A focused
//! field gets a document of its own ([`TextStoreHost::focus_store`]): a new
//! document manager and context over a [`TsfStore`] that reads and edits the
//! field's [`TextStore`] under the store's own locks. A grant captures its
//! document, so a lock deferred for one field never reaches the next.
//!
//! **Host operations never run under a TSF call.** A call from TSF into the
//! store can reach application code that moves focus or ends a composition;
//! doing that inline would pop or release the very document whose method is
//! still on the stack. Every COM entry and every host operation counts in
//! `entry_depth`; an operation that arrives while it is non-zero is queued
//! and runs when the outermost entry returns (explicitly, after its body).
//! A queued completion keeps the store it was asked for, and commits that
//! store's composition in place whenever TSF cannot end it.
//!
//! **Application code under a host operation is contained.** Retiring a
//! store's observer, dropping a store and committing in place run the
//! application's code; a panic there is caught, the operation's native
//! cleanup (association, focus, `Pop`) still runs and the queue behind it
//! still drains. The outermost host operation then raises the first such
//! failure; under a COM entry, which never unwinds into TSF, it is logged.
//!
//! Owner-thread only: nothing here is `Send` or `Sync`.

mod document;
#[cfg(test)]
mod probe;
#[cfg(test)]
mod tests;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use flui_foundation::panic::retain_opaque_payload;
use flui_platform_api::text_store::{
    CompositionEnd, OwnerCalls, TextStore, TextStoreHost, TextStoreHostError,
    commit_composition_in_place,
};
use windows::Win32::{
    Foundation::HWND,
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    UI::{
        Input::KeyboardAndMouse::GetFocus,
        TextServices::{
            CLSID_TF_ThreadMgr, ITextStoreACP, ITfContext, ITfContextOwnerCompositionServices,
            ITfDocumentMgr, ITfThreadMgr, TF_POPF_ALL,
        },
    },
};
use windows_core::{IUnknown, Interface};

use self::document::{DocumentState, TsfStore};

/// A host operation waiting for the COM entry it arrived under to return.
enum HostOp {
    Focus(Option<Rc<dyn TextStore>>),
    /// End the composition in this store, the one focused when the request
    /// was queued.
    CompleteComposition(Rc<dyn TextStore>),
    /// Replace a poisoned document with the empty one.
    DropPoisoned(Rc<DocumentState>),
}

/// The focused field's TSF document.
struct Document {
    manager: ITfDocumentMgr,
    context: ITfContext,
    state: Rc<DocumentState>,
    /// TSF holds the store through the context; this keeps our own handle.
    tsf_store: ITextStoreACP,
}

/// What a window's text services offer TSF.
enum Serving {
    /// No field: TSF sees the empty document.
    Nothing,
    /// The focused field's document.
    Field(Document),
    /// Shut down: TSF is deactivated for the window, and text input falls
    /// back to `WM_CHAR`.
    Shutdown,
}

/// One window's TSF connection; see the module doc.
pub(super) struct TextServices {
    hwnd: HWND,
    thread_manager: ITfThreadMgr,
    client_id: u32,
    empty: ITfDocumentMgr,
    serving: RefCell<Serving>,
    entry_depth: Cell<u32>,
    pending: RefCell<VecDeque<HostOp>>,
    poisonings: Cell<u8>,
    /// The first panic application code raised under a host operation (a
    /// store's observer retirement or destruction, a commit in place),
    /// caught so the operation's native cleanup still ran. The outermost
    /// host operation raises it once it is done; a COM entry, which must
    /// not unwind into TSF, logs it instead.
    failure: RefCell<Option<Box<dyn Any + Send>>>,
    me: Weak<TextServices>,
}

/// Resets `entry_depth` by one when a host operation's frame ends, unwinding
/// included; the queue itself is drained explicitly, never from here.
struct Depth<'a>(&'a Cell<u32>);

impl Drop for Depth<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

/// Associate `manager` with `hwnd` and release the manager it replaces.
/// A window with no previous association answers `S_OK` with no manager,
/// which the bindings report as an error carrying a success code.
///
/// # Safety
///
/// Owner thread of `hwnd`, inside its COM apartment.
unsafe fn associate(
    thread_manager: &ITfThreadMgr,
    hwnd: HWND,
    manager: Option<&ITfDocumentMgr>,
) -> windows_core::Result<()> {
    // SAFETY: the caller's contract; the returned previous manager is
    // AddRef'd for us and released by dropping it here.
    match unsafe { thread_manager.AssociateFocus(hwnd, manager) } {
        Ok(previous) => {
            drop(previous);
            Ok(())
        }
        Err(error) if error.code().is_ok() => Ok(()),
        Err(error) => Err(error),
    }
}

/// Whether two COM pointers name the same object.
fn same_object(a: &impl Interface, b: &impl Interface) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl TextServices {
    /// Activate TSF for `hwnd`'s thread and associate the empty document
    /// with the window. The thread must be the window's owner thread, inside
    /// a single-threaded COM apartment (the platform establishes both).
    ///
    /// # Errors
    ///
    /// The COM error of the step that failed; the caller falls back to the
    /// `WM_CHAR` path.
    pub(super) fn activate(hwnd: HWND) -> windows_core::Result<Rc<Self>> {
        // SAFETY: plain COM calls on this STA thread with no pointer
        // arguments of ours; each result is checked before the next step.
        let (thread_manager, client_id, empty) = unsafe {
            let thread_manager: ITfThreadMgr =
                CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER)?;
            let client_id = thread_manager.Activate()?;
            let empty = match thread_manager.CreateDocumentMgr() {
                Ok(empty) => empty,
                Err(error) => {
                    let _ = thread_manager.Deactivate();
                    return Err(error);
                }
            };
            if let Err(error) = associate(&thread_manager, hwnd, Some(&empty)) {
                let _ = thread_manager.Deactivate();
                return Err(error);
            }
            (thread_manager, client_id, empty)
        };
        tracing::debug!(target: "flui_platform::tsf", client_id, "ITfThreadMgr activated");
        Ok(Rc::new_cyclic(|me| Self {
            hwnd,
            thread_manager,
            client_id,
            empty,
            serving: RefCell::new(Serving::Nothing),
            entry_depth: Cell::new(0),
            pending: RefCell::new(VecDeque::new()),
            poisonings: Cell::new(0),
            failure: RefCell::new(None),
            me: me.clone(),
        }))
    }

    /// The client id `ITfThreadMgr::Activate` returned.
    pub(super) fn client_id(&self) -> u32 {
        self.client_id
    }

    /// The thread manager, for the window's own TSF queries.
    pub(super) fn thread_manager(&self) -> &ITfThreadMgr {
        &self.thread_manager
    }

    /// Whether TSF's focus is on the focused field's document.
    pub(super) fn document_has_focus(&self) -> bool {
        let manager = match &*self.serving.borrow() {
            Serving::Field(document) => document.manager.clone(),
            Serving::Nothing | Serving::Shutdown => return false,
        };
        // SAFETY: a plain COM call.
        let focus = unsafe { self.thread_manager.GetFocus() };
        focus.is_ok_and(|focus| same_object(&manager, &focus))
    }

    /// The focused document's state, for the window's diagnostics.
    fn focused_state(&self) -> Option<Rc<DocumentState>> {
        match &*self.serving.borrow() {
            Serving::Field(document) => Some(Rc::clone(&document.state)),
            Serving::Nothing | Serving::Shutdown => None,
        }
    }

    fn is_shut_down(&self) -> bool {
        matches!(*self.serving.borrow(), Serving::Shutdown)
    }

    /// The store the host was last told to focus: the newest queued focus
    /// change, else the open document's.
    fn focused_store(&self) -> Option<Rc<dyn TextStore>> {
        let queued = self.pending.borrow().iter().rev().find_map(|op| match op {
            HostOp::Focus(store) => Some(store.clone()),
            HostOp::CompleteComposition(_) | HostOp::DropPoisoned(_) => None,
        });
        queued.unwrap_or_else(|| self.focused_state().map(|state| Rc::clone(&state.store)))
    }

    /// A COM entry starts: host operations queue until it ends.
    fn enter(&self) {
        self.entry_depth.set(self.entry_depth.get() + 1);
    }

    /// A COM entry ended; the outermost one runs what was queued under it.
    /// A failure those operations caught cannot be handed back through TSF:
    /// it is logged, not raised.
    fn leave(&self) {
        self.entry_depth
            .set(self.entry_depth.get().saturating_sub(1));
        self.drain_pending();
        if self.entry_depth.get() == 0
            && let Some(payload) = self.take_failure()
        {
            tracing::error!(
                target: "flui_platform::tsf",
                panic = crate::shared::panic_boundary::panic_payload_message(&*payload),
                "application code a TSF document's teardown reached panicked"
            );
            retain_opaque_payload(payload);
        }
    }

    /// Keep `payload` for the outermost host operation; a later failure is
    /// retained behind the first.
    fn keep_failure(&self, payload: Box<dyn Any + Send>) {
        let mut failure = self.failure.borrow_mut();
        if failure.is_none() {
            *failure = Some(payload);
        } else {
            drop(failure);
            retain_opaque_payload(payload);
        }
    }

    fn take_failure(&self) -> Option<Box<dyn Any + Send>> {
        self.failure.borrow_mut().take()
    }

    /// Raise a failure the host operation that is returning caught, once no
    /// other operation or COM entry is still running under it.
    fn raise_failure(&self) {
        if self.entry_depth.get() == 0
            && let Some(payload) = self.take_failure()
        {
            resume_unwind(payload);
        }
    }

    /// A call into `state`'s document panicked: switch to the empty
    /// document once the entry returns; after three, stop offering text
    /// input to TSF altogether.
    fn document_poisoned(&self, state: &Rc<DocumentState>) {
        self.poisonings.set(self.poisonings.get().saturating_add(1));
        self.pending
            .borrow_mut()
            .push_back(HostOp::DropPoisoned(Rc::clone(state)));
    }

    fn enter_host_op(&self) -> Depth<'_> {
        self.entry_depth.set(self.entry_depth.get() + 1);
        Depth(&self.entry_depth)
    }

    fn run_host_op(&self, op: HostOp) {
        if self.entry_depth.get() > 0 {
            self.pending.borrow_mut().push_back(op);
            return;
        }
        let depth = self.enter_host_op();
        self.apply_contained(op);
        drop(depth);
        self.drain_pending();
    }

    fn drain_pending(&self) {
        while self.entry_depth.get() == 0 {
            let Some(op) = self.pending.borrow_mut().pop_front() else {
                return;
            };
            let depth = self.enter_host_op();
            self.apply_contained(op);
            drop(depth);
        }
    }

    /// Apply `op`, keeping a panic of the application code it reaches for
    /// the outermost host operation, so the queue behind it still runs.
    fn apply_contained(&self, op: HostOp) {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| self.apply(op))) {
            self.keep_failure(payload);
        }
    }

    fn apply(&self, op: HostOp) {
        if self.is_shut_down() {
            // A queued completion was answered `Deferred`: with TSF gone,
            // the composition is still committed, in place.
            if let HostOp::CompleteComposition(store) = op {
                commit_composition_in_place(&*store);
            }
            return;
        }
        match op {
            HostOp::Focus(store) => self.apply_focus(store),
            HostOp::CompleteComposition(store) => {
                // Nobody waits for this answer, so whatever TSF did not
                // commit (no document for the store, or a refused lock) is
                // committed in place here, keeping the text.
                if self.terminate_composition(&store) != Some(CompositionEnd::Committed) {
                    commit_composition_in_place(&*store);
                }
            }
            HostOp::DropPoisoned(state) => {
                let poisoned_is_focused = matches!(
                    &*self.serving.borrow(),
                    Serving::Field(document) if Rc::ptr_eq(&document.state, &state)
                );
                if poisoned_is_focused {
                    self.close_document();
                }
                if self.poisonings.get() >= 3 {
                    tracing::warn!(
                        target: "flui_platform::tsf",
                        "three TSF documents poisoned in this window; text input falls back to WM_CHAR"
                    );
                    self.shutdown();
                }
            }
        }
    }

    fn apply_focus(&self, store: Option<Rc<dyn TextStore>>) {
        let unchanged = match (&*self.serving.borrow(), &store) {
            (Serving::Field(document), Some(store)) => Rc::ptr_eq(&document.state.store, store),
            (Serving::Nothing, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        self.close_document();
        if let Some(store) = store
            && let Err(error) = self.open_document(store)
        {
            tracing::warn!(target: "flui_platform::tsf", ?error, "could not open a TSF document");
        }
    }

    fn open_document(&self, store: Rc<dyn TextStore>) -> windows_core::Result<()> {
        let state = DocumentState::new(store, self.hwnd, self.me.clone());
        let tsf_store: ITextStoreACP = TsfStore::new(Rc::clone(&state)).into();
        let mut context = None;
        let mut edit_cookie = 0;
        // SAFETY: plain COM calls on this STA thread; `context` and
        // `edit_cookie` are live, writable locals for `CreateContext`.
        let (manager, context) = unsafe {
            let manager = self.thread_manager.CreateDocumentMgr()?;
            manager.CreateContext(
                self.client_id,
                0,
                &tsf_store,
                &raw mut context,
                &raw mut edit_cookie,
            )?;
            let context = context.ok_or(windows::Win32::Foundation::E_UNEXPECTED)?;
            manager.Push(&context)?;
            // The manager `AssociateFocus` returns (the empty one) is
            // released here, by dropping it.
            associate(&self.thread_manager, self.hwnd, Some(&manager))?;
            (manager, context)
        };
        let associated = self.focus_is(&manager);
        // SAFETY: `GetFocus` takes no arguments.
        let window_focused = unsafe { GetFocus() } == self.hwnd;
        if window_focused {
            // SAFETY: a plain COM call on this STA thread.
            unsafe { self.thread_manager.SetFocus(&manager) }?;
        }
        tracing::debug!(
            target: "flui_platform::tsf",
            associated_moved_focus = associated,
            window_focused,
            focused_after = self.focus_is(&manager),
            "TSF document opened"
        );
        *self.serving.borrow_mut() = Serving::Field(Document {
            manager,
            context,
            state,
            tsf_store,
        });
        Ok(())
    }

    fn focus_is(&self, manager: &ITfDocumentMgr) -> bool {
        // SAFETY: a plain COM call.
        unsafe { self.thread_manager.GetFocus() }.is_ok_and(|focus| same_object(manager, &focus))
    }

    /// Close the focused document, if any, and serve the empty one.
    fn close_document(&self) {
        let previous = {
            let mut serving = self.serving.borrow_mut();
            match *serving {
                Serving::Field(_) => std::mem::replace(&mut *serving, Serving::Nothing),
                Serving::Nothing | Serving::Shutdown => return,
            }
        };
        if let Serving::Field(document) = previous {
            self.release_document(document);
        }
    }

    /// TSF focus back to the empty manager, then pop and release
    /// everything of `document`.
    ///
    /// Retiring the store's observer and dropping the document's hold on the
    /// store run application code: a panic there is kept for the outermost
    /// host operation ([`Self::keep_failure`]), after which the native
    /// cleanup still runs, so TSF is never left associated with a document
    /// the window no longer serves. After a failure the store is retained,
    /// not destroyed (ADR-0127).
    fn release_document(&self, document: Document) {
        let Document {
            manager,
            context,
            state,
            tsf_store,
        } = document;
        let mut calls = OwnerCalls::new();
        calls.run(|| state.close());
        // SAFETY: plain COM calls on this STA thread.
        unsafe {
            let _ = associate(&self.thread_manager, self.hwnd, Some(&self.empty));
            if GetFocus() == self.hwnd {
                let _ = self.thread_manager.SetFocus(&self.empty);
            }
            if let Err(error) = manager.Pop(TF_POPF_ALL) {
                tracing::debug!(target: "flui_platform::tsf", ?error, "Pop failed");
            }
        }
        drop((context, tsf_store, manager));
        calls.retire(state);
        if let Some(payload) = calls.into_failure() {
            self.keep_failure(payload);
        }
    }

    /// End TSF's composition in `store`'s document; `None` when `store` has
    /// no open document. A refusal replaces the document, which discards
    /// TSF's composition, and answers `Abandoned`.
    fn terminate_composition(&self, store: &Rc<dyn TextStore>) -> Option<CompositionEnd> {
        let context = match &*self.serving.borrow() {
            Serving::Field(document) if Rc::ptr_eq(&document.state.store, store) => {
                document.context.clone()
            }
            _ => return None,
        };
        let services: windows_core::Result<ITfContextOwnerCompositionServices> = context.cast();
        // SAFETY: a plain COM call; `None` asks TSF to end every composition.
        let terminated = services.and_then(|services| unsafe {
            services
                .TerminateComposition(None::<&windows::Win32::UI::TextServices::ITfCompositionView>)
        });
        match terminated {
            Ok(()) => {
                tracing::debug!(target: "flui_platform::tsf", "TerminateComposition committed");
                Some(CompositionEnd::Committed)
            }
            Err(error) => {
                tracing::debug!(
                    target: "flui_platform::tsf",
                    ?error,
                    "TerminateComposition refused; the document is replaced"
                );
                self.close_document();
                if let Err(error) = self.open_document(Rc::clone(store)) {
                    tracing::warn!(target: "flui_platform::tsf", ?error, "could not reopen the TSF document");
                }
                Some(CompositionEnd::Abandoned)
            }
        }
    }

    /// Close the document, release the empty one and deactivate TSF for
    /// the window. Idempotent.
    pub(super) fn shutdown(&self) {
        match self.serving.replace(Serving::Shutdown) {
            Serving::Shutdown => return,
            Serving::Field(document) => self.release_document(document),
            Serving::Nothing => {}
        }
        // SAFETY: plain COM calls on this STA thread.
        unsafe {
            let _ = associate(&self.thread_manager, self.hwnd, None);
            if let Err(error) = self.thread_manager.Deactivate() {
                tracing::debug!(target: "flui_platform::tsf", ?error, "Deactivate failed");
            }
        }
    }
}

impl TextStoreHost for TextServices {
    /// Queued when called under a TSF call into a store.
    ///
    /// # Panics
    ///
    /// Resumes the first panic of application code the change reached (the
    /// previous store's observer retirement), once TSF serves the new
    /// document or the empty one.
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        self.run_host_op(HostOp::Focus(store));
        self.raise_failure();
    }

    /// `Deferred` when the request is queued behind a TSF call in progress:
    /// the queue keeps `store` and commits its composition in place if TSF
    /// then cannot end it, or has shut down.
    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        if self.is_shut_down() {
            return Err(TextStoreHostError::Unavailable);
        }
        if !self
            .focused_store()
            .is_some_and(|focused| Rc::ptr_eq(&focused, store))
        {
            return Err(TextStoreHostError::NotFocused);
        }
        if self.entry_depth.get() > 0 {
            self.pending
                .borrow_mut()
                .push_back(HostOp::CompleteComposition(Rc::clone(store)));
            return Ok(CompositionEnd::Deferred);
        }
        let depth = self.enter_host_op();
        let end = catch_unwind(AssertUnwindSafe(|| self.terminate_composition(store)))
            .unwrap_or_else(|payload| {
                self.keep_failure(payload);
                None
            });
        drop(depth);
        self.drain_pending();
        if self.entry_depth.get() == 0 && self.failure.borrow().is_some() {
            // The answer does not reach the caller: what TSF did not commit
            // is committed in place before the failure is raised.
            if end != Some(CompositionEnd::Committed)
                && let Err(payload) =
                    catch_unwind(AssertUnwindSafe(|| commit_composition_in_place(&**store)))
            {
                self.keep_failure(payload);
            }
            self.raise_failure();
        }
        // Focused but without a document: opening it failed, so TSF holds
        // no composition there.
        end.ok_or(TextStoreHostError::NotFocused)
    }
}

impl Drop for TextServices {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(payload) = self.take_failure() {
            if std::thread::panicking() {
                retain_opaque_payload(payload);
            } else {
                resume_unwind(payload);
            }
        }
    }
}
