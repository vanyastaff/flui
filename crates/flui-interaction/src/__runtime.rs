//! Presentation-owner terminal cleanup. No application API or semver promise.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{FocusManager, TextInputOwner, retain::Retain};

/// Whether terminal cleanup already owes an earlier failure to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseMode {
    /// Deliver final notifications and ordinarily destroy outgoing ownership.
    Ordinary,
    /// Revoke authority, but retain outgoing opaque ownership and omit optional callbacks.
    PreservingFailure,
}

/// Give one presentation independently withdrawable registration authority.
pub fn presentation_dispatch(
    handle: &crate::InteractionDispatchHandle,
) -> crate::InteractionDispatchHandle {
    handle.scoped_owner()
}

/// Withdraw a presentation's registered targets on the owner thread.
pub fn close_dispatch(handle: &crate::InteractionDispatchHandle, mode: CloseMode) {
    handle.close_owner(mode);
}

/// Withdraw through the physical lane during realm destruction, without TLS activation.
pub fn close_dispatch_in(
    lane: &crate::InteractionLane,
    handle: &crate::InteractionDispatchHandle,
    mode: CloseMode,
) {
    handle.close_owner_in(lane, mode);
}

/// Close the focus owner under the presentation's current recovery policy.
pub fn close_focus(owner: &FocusManager, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// Close text input under the presentation's current recovery policy.
pub fn close_text_input(owner: &TextInputOwner, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// Withdraw cached routes and gesture admission for a terminal presentation.
pub fn close_gestures(owner: &crate::GestureBinding, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// Withdraw mouse publication and callback ownership for a terminal presentation.
pub fn close_mouse_tracker(owner: &crate::routing::MouseTracker, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// A terminal mode outlives an owner without retaining that owner or its callbacks.
#[derive(Clone, Debug)]
pub(crate) struct CloseTombstone(Arc<AtomicBool>);

impl Default for CloseTombstone {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

impl CloseTombstone {
    pub(crate) fn mode(&self) -> CloseMode {
        if self.0.load(Ordering::Acquire) {
            CloseMode::PreservingFailure
        } else {
            CloseMode::Ordinary
        }
    }
    pub(crate) fn preserve(&self) {
        self.0.store(true, Ordering::Release);
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
            || terminal.mode() == CloseMode::PreservingFailure;
        if preserving {
            terminal.preserve();
        }
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
                if let Some(terminal) = &self.terminal {
                    terminal.preserve();
                }
                if self.preserving() {
                    flui_foundation::panic::retain_opaque_payload(payload);
                } else {
                    self.first = Some(payload);
                }
                None
            }
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

    pub(crate) fn finish(self) {
        if let Some(payload) = self.first {
            resume_unwind(payload);
        }
    }

    pub(crate) fn finish_contained(self) {
        if let Some(payload) = self.first {
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
