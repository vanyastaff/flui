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

use crate::retain::Retain;

/// Whether terminal cleanup already owes an earlier failure to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseMode {
    /// Deliver final notifications and ordinarily destroy outgoing ownership.
    Ordinary,
    /// Revoke authority, but retain outgoing opaque ownership and omit optional callbacks.
    PreservingFailure,
}

/// A terminal mode outlives an owner without retaining that owner or its callbacks.
///
/// Two policies hang off it. Values a preserving close withdrew stay under
/// retention wherever their last owner later lands ([`Self::preserved`]).
/// Values offered to or invoked through the closed owner afterwards follow
/// [`Self::mode`], which is preserving only while a close of this owner is in
/// progress, so a destructor that reenters during that close cannot destroy
/// what the close owes its failure; once the close returns and its failure is
/// caught, rejections through stale handles retire normally again.
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
    /// one: the first is raised by an ordinary close, the rest retained.
    pub(crate) fn keep_caught(&mut self, payload: Box<dyn Any + Send>) {
        if let Some(terminal) = &self.terminal {
            terminal.preserve();
        }
        if self.preserving() {
            flui_foundation::panic::retain_opaque_payload(payload);
        } else {
            self.first = Some(payload);
        }
    }

    /// Destroy `value` ordinarily, or, once a failure is owed, retain only what
    /// its destruction would run as user code ([`Retain`]): a shared handle
    /// that is not the last owner is released, so a still-live owner keeps
    /// sole custody of its captures (ADR-0127).
    pub(crate) fn retire<T: Retain>(&mut self, value: T) {
        if self.preserving() {
            value.retain();
        } else {
            self.run(|| drop(value));
        }
    }

    /// Release a framework-owned handle (a platform capability): dropping it
    /// runs no user code, so it is destroyed even after a failure. Only an
    /// unwind already in progress retains it (ADR-0127).
    pub(crate) fn release<T>(&mut self, value: T) {
        if std::thread::panicking() {
            std::mem::forget(value);
        } else {
            let _ = self.invoke(|| drop(value));
        }
    }

    /// Keep `payload`, a failure caught before this close began (one a store
    /// parked for its presentation), ahead of the close's own: raised by an
    /// ordinary close, retained by a preserving one (ADR-0123).
    pub(crate) fn keep_earlier(&mut self, payload: Box<dyn Any + Send>) {
        if self.preserving {
            flui_foundation::panic::retain_opaque_payload(payload);
            return;
        }
        if let Some(later) = self.first.replace(payload) {
            flui_foundation::panic::retain_opaque_payload(later);
        }
    }

    pub(crate) fn finish(mut self) {
        if let Some(payload) = self.first.take() {
            resume_unwind(payload);
        }
    }

    pub(crate) fn finish_contained(mut self) {
        if let Some(payload) = self.first.take() {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }

    pub(crate) fn finish_with<T: Default + Retain>(self, value: T) -> T {
        if self.preserving() {
            value.retain();
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
