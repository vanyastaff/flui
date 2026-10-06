//! The Win32 text services: TSF, the input-method path of a FLUI window
//! (ADR-0090 §3; no IMM32).
//!
//! One [`TextServices`] per window activates the thread's `ITfThreadMgr`
//! and associates an empty document manager with the window, so an input
//! method sees "no editable field" until a field takes focus. A focused
//! field gets a document of its own ([`TextServices::focus_store`]): a new
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
//!
//! Owner-thread only: nothing here is `Send` or `Sync`.

mod document;
#[cfg(test)]
mod probe;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use flui_platform_api::text_store::TextStore;
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

/// How a request to end the focused field's composition ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompositionEnd {
    /// The input method committed its composition into the store.
    Committed,
    /// The input method could not (its lock was refused): its composition
    /// is discarded with the document, and the caller clears the store's
    /// composition itself, keeping the text.
    Abandoned,
}

/// A host operation waiting for the COM entry it arrived under to return.
enum HostOp {
    Focus(Option<Rc<dyn TextStore>>),
    CompleteComposition,
    /// Replace a poisoned document with the empty one.
    DropPoisoned(Rc<DocumentState>),
}

/// The focused field's TSF document.
struct Document {
    manager: ITfDocumentMgr,
    context: ITfContext,
    state: Rc<DocumentState>,
    /// TSF holds the store through the context; this keeps our own handle.
    _store: ITextStoreACP,
}

/// One window's TSF connection; see the module doc.
pub(super) struct TextServices {
    hwnd: HWND,
    thread_manager: ITfThreadMgr,
    client_id: u32,
    empty: ITfDocumentMgr,
    document: RefCell<Option<Document>>,
    entry_depth: Cell<u32>,
    pending: RefCell<VecDeque<HostOp>>,
    poisonings: Cell<u8>,
    active: Cell<bool>,
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
            document: RefCell::new(None),
            entry_depth: Cell::new(0),
            pending: RefCell::new(VecDeque::new()),
            poisonings: Cell::new(0),
            active: Cell::new(true),
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
        let manager = self
            .document
            .borrow()
            .as_ref()
            .map(|document| document.manager.clone());
        // SAFETY: a plain COM call.
        let focus = unsafe { self.thread_manager.GetFocus() };
        matches!((manager, focus), (Some(manager), Ok(focus)) if same_object(&manager, &focus))
    }

    /// The focused document's state, for the window's diagnostics.
    fn focused_state(&self) -> Option<Rc<DocumentState>> {
        self.document
            .borrow()
            .as_ref()
            .map(|document| Rc::clone(&document.state))
    }

    /// `store` now receives this window's text input; `None` when no field
    /// does. Queued when called under a TSF call into a store.
    pub(super) fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        self.run_host_op(HostOp::Focus(store));
    }

    /// End the focused field's composition. `None` when the request was
    /// queued behind a TSF call in progress, or there is no document.
    pub(super) fn complete_composition(&self) -> Option<CompositionEnd> {
        if self.entry_depth.get() > 0 {
            self.pending
                .borrow_mut()
                .push_back(HostOp::CompleteComposition);
            return None;
        }
        let depth = self.enter_host_op();
        let end = self.terminate_composition();
        drop(depth);
        self.drain_pending();
        end
    }

    /// A COM entry starts: host operations queue until it ends.
    fn enter(&self) {
        self.entry_depth.set(self.entry_depth.get() + 1);
    }

    /// A COM entry ended; the outermost one runs what was queued under it.
    fn leave(&self) {
        self.entry_depth
            .set(self.entry_depth.get().saturating_sub(1));
        self.drain_pending();
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
        self.apply(op);
        drop(depth);
        self.drain_pending();
    }

    fn drain_pending(&self) {
        while self.entry_depth.get() == 0 {
            let Some(op) = self.pending.borrow_mut().pop_front() else {
                return;
            };
            let depth = self.enter_host_op();
            self.apply(op);
            drop(depth);
        }
    }

    fn apply(&self, op: HostOp) {
        if !self.active.get() {
            return;
        }
        match op {
            HostOp::Focus(store) => self.apply_focus(store),
            HostOp::CompleteComposition => {
                let _ = self.terminate_composition();
            }
            HostOp::DropPoisoned(state) => {
                let poisoned_is_focused = self
                    .document
                    .borrow()
                    .as_ref()
                    .is_some_and(|document| Rc::ptr_eq(&document.state, &state));
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
        let unchanged = {
            let document = self.document.borrow();
            match (&*document, &store) {
                (Some(document), Some(store)) => Rc::ptr_eq(&document.state.store, store),
                (None, None) => true,
                _ => false,
            }
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
        *self.document.borrow_mut() = Some(Document {
            manager,
            context,
            state,
            _store: tsf_store,
        });
        Ok(())
    }

    fn focus_is(&self, manager: &ITfDocumentMgr) -> bool {
        // SAFETY: a plain COM call.
        unsafe { self.thread_manager.GetFocus() }.is_ok_and(|focus| same_object(manager, &focus))
    }

    /// Close the focused document: TSF focus back to the empty manager,
    /// then pop and release everything.
    fn close_document(&self) {
        let Some(document) = self.document.borrow_mut().take() else {
            return;
        };
        document.state.close();
        // SAFETY: plain COM calls on this STA thread.
        unsafe {
            let _ = associate(&self.thread_manager, self.hwnd, Some(&self.empty));
            if GetFocus() == self.hwnd {
                let _ = self.thread_manager.SetFocus(&self.empty);
            }
            if let Err(error) = document.manager.Pop(TF_POPF_ALL) {
                tracing::debug!(target: "flui_platform::tsf", ?error, "Pop failed");
            }
        }
        drop(document);
    }

    fn terminate_composition(&self) -> Option<CompositionEnd> {
        let (context, store) = {
            let document = self.document.borrow();
            let document = document.as_ref()?;
            (document.context.clone(), Rc::clone(&document.state.store))
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
                if let Err(error) = self.open_document(store) {
                    tracing::warn!(target: "flui_platform::tsf", ?error, "could not reopen the TSF document");
                }
                Some(CompositionEnd::Abandoned)
            }
        }
    }

    /// Close the document, release the empty one and deactivate TSF for
    /// the window. Idempotent.
    pub(super) fn shutdown(&self) {
        if !self.active.replace(false) {
            return;
        }
        self.close_document();
        // SAFETY: plain COM calls on this STA thread.
        unsafe {
            let _ = associate(&self.thread_manager, self.hwnd, None);
            if let Err(error) = self.thread_manager.Deactivate() {
                tracing::debug!(target: "flui_platform::tsf", ?error, "Deactivate failed");
            }
        }
    }
}

impl Drop for TextServices {
    fn drop(&mut self) {
        self.shutdown();
    }
}
