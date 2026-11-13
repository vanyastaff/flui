//! Terminal close policy shared by every owner a presentation closes.
//!
//! [`CloseMode`] says whether a close already owes a failure; a
//! [`CloseTombstone`] outlives its owner to carry that policy to later
//! rejections; [`ClosePanic`] owns one close's first failure.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// Whether terminal cleanup already owes an earlier failure to its caller.
///
/// Either way outgoing ownership is destroyed; after a failure each
/// destructor runs under the close's containment (ADR-0281 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a close runs ordinary or preserving a failure, and a `_` arm would skip its cleanup"
)]
pub enum CloseMode {
    /// Deliver final notifications and destroy outgoing ownership.
    Ordinary,
    /// Revoke authority and omit optional callbacks; a panic of an outgoing
    /// destructor is discarded behind the failure already owed.
    PreservingFailure,
}

/// A terminal mode outlives an owner without retaining that owner or its callbacks.
///
/// Two policies hang off it. A close of an owner whose earlier close ran, or
/// turned, preserving is preserving too ([`Self::preserved`]), so its
/// optional callbacks stay skipped. Values offered to or invoked through the
/// closed owner afterwards follow [`Self::mode`], which is preserving only
/// while a close of this owner is in progress, so a panic from a destructor
/// that reenters during that close is discarded behind the failure the close
/// owes (ADR-0281 §3); once the close returns and its failure is caught,
/// rejections through stale handles retire as ordinary ones again.
#[derive(Clone, Debug, Default)]
pub(crate) struct CloseTombstone(Arc<TombstoneState>);

#[derive(Debug, Default)]
struct TombstoneState {
    preserved: AtomicBool,
    closing: AtomicUsize,
}

impl CloseTombstone {
    /// The policy for values rejected by, or run through, the closed owner.
    pub(crate) fn mode(&self) -> CloseMode {
        if self.0.closing.load(Ordering::Acquire) > 0 && self.preserved() {
            CloseMode::PreservingFailure
        } else {
            CloseMode::Ordinary
        }
    }

    /// Whether a close of this owner ran, or turned, preserving.
    pub(crate) fn preserved(&self) -> bool {
        self.0.preserved.load(Ordering::Acquire)
    }

    pub(crate) fn preserve(&self) {
        self.0.preserved.store(true, Ordering::Release);
    }

    pub(crate) fn enter_close(&self) {
        self.0.closing.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn exit_close(&self) {
        self.0.closing.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One close owns its first failure and decides retirement before calling Drop.
pub(crate) struct ClosePanic {
    first: Option<Box<dyn Any + Send>>,
    preserving: bool,
    terminal: Option<CloseTombstone>,
}

impl ClosePanic {
    pub(crate) fn new() -> Self {
        Self {
            first: None,
            preserving: std::thread::panicking(),
            terminal: None,
        }
    }

    pub(crate) fn for_close(mode: CloseMode, terminal: CloseTombstone) -> Self {
        let preserving = mode == CloseMode::PreservingFailure
            || std::thread::panicking()
            || terminal.preserved();
        if preserving {
            terminal.preserve();
        }
        terminal.enter_close();
        Self {
            first: None,
            preserving,
            terminal: Some(terminal),
        }
    }

    pub(crate) fn for_rejection(mode: CloseMode) -> Self {
        let mut state = Self::new();
        state.preserving |= mode == CloseMode::PreservingFailure;
        state
    }

    /// Continue this close under the caller's current mode: a failure the
    /// caller caught since the close began makes the rest of it preserving.
    pub(crate) fn adopt(&mut self, mode: CloseMode) {
        if mode == CloseMode::PreservingFailure || std::thread::panicking() {
            self.preserving = true;
            if let Some(terminal) = &self.terminal {
                terminal.preserve();
            }
        }
    }

    pub(crate) fn preserving(&self) -> bool {
        self.preserving || self.first.is_some()
    }

    pub(crate) fn run(&mut self, run: impl FnOnce()) {
        if !self.preserving() {
            let _ = self.invoke(run);
        }
    }

    /// Required quiescence may run during recovery; its ownership stays outside this catch.
    pub(crate) fn invoke<T>(&mut self, run: impl FnOnce() -> T) -> Option<T> {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: required quiescence runs contained; the first failure is kept"
        )]
        match catch_unwind(AssertUnwindSafe(run)) {
            Ok(value) => Some(value),
            Err(payload) => {
                self.keep_caught(payload);
                None
            }
        }
    }

    /// Keep `payload`, a failure of this close's own code the caller caught
    /// (in the order its own containment decided), as [`Self::invoke`] keeps
    /// one: the first is raised by an ordinary close, the rest discarded.
    pub(crate) fn keep_caught(&mut self, payload: Box<dyn Any + Send>) {
        if let Some(terminal) = &self.terminal {
            terminal.preserve();
        }
        if self.preserving() {
            flui_foundation::panic::discard_payload(payload);
        } else {
            self.first = Some(payload);
        }
    }

    /// Destroy `value` under this close's containment: a panic of its
    /// destructor becomes the close's first failure, or is discarded behind
    /// one already owed (ADR-0281 §3).
    pub(crate) fn retire<T>(&mut self, value: T) {
        let _: Option<()> = self.invoke(|| drop(value));
    }

    /// Keep `payload`, a failure caught before this close began (one a store
    /// parked for its presentation), ahead of the close's own: raised by an
    /// ordinary close, discarded by a preserving one (ADR-0123, ADR-0281 §2).
    pub(crate) fn keep_earlier(&mut self, payload: Box<dyn Any + Send>) {
        if self.preserving {
            flui_foundation::panic::discard_payload(payload);
            return;
        }
        if let Some(later) = self.first.replace(payload) {
            flui_foundation::panic::discard_payload(later);
        }
    }

    pub(crate) fn finish(mut self) {
        if let Some(payload) = self.first.take() {
            resume_unwind(payload);
        }
    }

    pub(crate) fn finish_contained(mut self) {
        if let Some(payload) = self.first.take() {
            flui_foundation::panic::discard_payload(payload);
        }
    }

    /// Hand `value` back after an ordinary close; a preserving close destroys
    /// it under containment instead and raises its first failure.
    pub(crate) fn finish_with<T: Default>(mut self, value: T) -> T {
        if self.preserving() {
            self.retire(value);
            self.finish();
            T::default()
        } else {
            value
        }
    }
}

impl Drop for ClosePanic {
    /// Ends this close's reentry window, including when its failure unwinds.
    fn drop(&mut self) {
        if let Some(terminal) = &self.terminal {
            terminal.exit_close();
        }
    }
}
