//! A recording pull-model host (ADR-0135): what a headless window offers in
//! place of the Win32 text services.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_platform_api::text_store::{
    CompositionEnd, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreHost,
    TextStoreHostError,
};

/// One call a presentation made on its [`RecordingTextStoreHost`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreHostCall {
    /// `focus_store(Some(_))`: a field's store now takes input.
    Focus,
    /// `focus_store(None)`: no field does.
    Unfocus,
    /// `complete_composition(store)`, whatever the answer.
    CompleteComposition,
}

/// A [`TextStoreHost`] that records every call and serves the focused store
/// the way a text service would.
///
/// Ending a composition commits it with a synchronous read-write lock on the
/// focused store, as a text service terminating its composition edits the
/// document; when the store refuses that lock (the frame transaction is
/// open, or a lock is held) it answers [`CompositionEnd::Abandoned`], as the
/// Win32 text services do. A store other than the focused one answers
/// [`TextStoreHostError::NotFocused`]. It also records a call that arrives while
/// another is running, which the presentation's owner must never make.
#[derive(Default)]
pub struct RecordingTextStoreHost {
    calls: RefCell<Vec<StoreHostCall>>,
    focused: RefCell<Option<Rc<dyn TextStore>>>,
    depth: Cell<u32>,
    nested: Cell<bool>,
    unavailable: Cell<bool>,
}

impl std::fmt::Debug for RecordingTextStoreHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingTextStoreHost")
            .field("calls", &self.calls.borrow())
            .field("focused", &self.focused.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl RecordingTextStoreHost {
    /// A host with no focused store and nothing recorded.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Every call, in the order it arrived.
    #[must_use]
    pub fn calls(&self) -> Vec<StoreHostCall> {
        self.calls.borrow().clone()
    }

    /// The store the last focus call named.
    #[must_use]
    pub fn focused_store(&self) -> Option<Rc<dyn TextStore>> {
        self.focused.borrow().clone()
    }

    /// Whether any call arrived while another was still running.
    #[must_use]
    pub fn was_reentered(&self) -> bool {
        self.nested.get()
    }

    /// From now on, answer completions with
    /// [`TextStoreHostError::Unavailable`], as a window whose text services
    /// shut down does.
    pub fn shut_down(&self) {
        self.unavailable.set(true);
    }

    fn enter(&self, call: StoreHostCall) -> Depth<'_> {
        if self.depth.get() > 0 {
            self.nested.set(true);
        }
        self.calls.borrow_mut().push(call);
        self.depth.set(self.depth.get() + 1);
        Depth(&self.depth)
    }
}

struct Depth<'a>(&'a Cell<u32>);

impl Drop for Depth<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl TextStoreHost for RecordingTextStoreHost {
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>) {
        let call = if store.is_some() {
            StoreHostCall::Focus
        } else {
            StoreHostCall::Unfocus
        };
        let _depth = self.enter(call);
        let previous = self.focused.replace(store);
        drop(previous);
    }

    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError> {
        let _depth = self.enter(StoreHostCall::CompleteComposition);
        if self.unavailable.get() {
            return Err(TextStoreHostError::Unavailable);
        }
        let focused = self
            .focused
            .borrow()
            .as_ref()
            .is_some_and(|focused| Rc::ptr_eq(focused, store));
        if !focused {
            return Err(TextStoreHostError::NotFocused);
        }
        let grant = LockGrant::read_write(|session| {
            if session.composition().is_some() {
                let _ = session.set_composition(None);
            }
        });
        Ok(match store.request_lock(grant, LockTiming::Sync) {
            Ok(LockOutcome::Granted) => CompositionEnd::Committed,
            _ => CompositionEnd::Abandoned,
        })
    }
}
