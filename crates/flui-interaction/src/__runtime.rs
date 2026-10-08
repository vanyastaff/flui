//! Presentation-owner terminal cleanup. No application API or semver promise.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub use crate::routing::DispatchCustody;
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

/// Withdraw a presentation's dispatch authority on the owner thread, keeping
/// its captures until [`retire_dispatch`]: a presentation closes every other
/// capability before any of them is destroyed (ADR-0123).
pub fn withdraw_dispatch(
    handle: &crate::InteractionDispatchHandle,
    mode: CloseMode,
) -> Option<DispatchCustody> {
    handle.withdraw_owner(mode)
}

/// [`withdraw_dispatch`] through the physical lane, without TLS activation.
pub fn withdraw_dispatch_in(
    lane: &crate::InteractionLane,
    handle: &crate::InteractionDispatchHandle,
    mode: CloseMode,
) -> Option<DispatchCustody> {
    handle.withdraw_owner_in(lane, mode)
}

/// Destroy what [`withdraw_dispatch`] withdrew, or retain it in preserving mode.
pub fn retire_dispatch(custody: DispatchCustody, mode: CloseMode) {
    custody.retire(mode);
}

/// Withdraw through the physical lane during UI runtime destruction, without TLS activation.
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

/// Refuse later focus requests without running user code; [`close_focus`]
/// still retires the owner. A UI runtime closing several presentations withdraws
/// every one before any of them runs a callback (ADR-0123).
pub fn withdraw_focus(owner: &FocusManager) {
    owner.withdraw();
}

/// Refuse later text-input callers without running user code;
/// [`close_text_input`] still disables the platform and retires the clients.
pub fn withdraw_text_input(owner: &TextInputOwner) {
    owner.withdraw();
}

/// Close text input under the presentation's current recovery policy.
pub fn close_text_input(owner: &TextInputOwner, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// Withdraw cached routes and gesture admission for a terminal presentation.
pub fn close_gestures(owner: &crate::GestureBinding, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// Install the owning presentation's weak redraw capability for logical capture release.
pub fn set_pointer_capture_wake(
    owner: &crate::GestureBinding,
    window: std::sync::Weak<dyn flui_platform_api::PlatformWindow>,
) {
    owner.set_pointer_capture_wake(window);
}

/// Withdraw mouse publication and callback ownership for a terminal presentation.
pub fn close_mouse_tracker(owner: &crate::routing::MouseTracker, mode: CloseMode) {
    owner.close_with_mode(mode);
}

/// The reentry window of one presentation close, across every owner it closes.
///
/// A destructor or platform callback run by one owner's close can reenter
/// another owner that closed earlier in the same presentation close. While
/// this window is held, such a rejection follows the close's retention policy
/// once [`Self::preserve`] marked it preserving; after the window is dropped,
/// rejections through stale handles retire normally again.
#[derive(Debug, Default)]
pub struct CloseWindow {
    terminals: Vec<CloseTombstone>,
}

impl CloseWindow {
    /// An empty window; add each owner the presentation closes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Hold the window of a presentation's dispatch owner.
    pub fn dispatch(&mut self, handle: &crate::InteractionDispatchHandle) {
        if let Some(terminal) = handle.close_tombstone() {
            self.hold(terminal);
        }
    }

    /// Hold the window of a presentation's focus owner.
    pub fn focus(&mut self, owner: &FocusManager) {
        self.hold(owner.close_tombstone());
    }

    /// Hold the window of a presentation's text-input owner.
    pub fn text_input(&mut self, owner: &TextInputOwner) {
        self.hold(owner.close_tombstone());
    }

    /// Hold the windows of a presentation's gesture binding, its arena,
    /// pointer router and mouse tracker.
    pub fn gestures(&mut self, owner: &crate::GestureBinding) {
        for terminal in owner.close_tombstones() {
            self.hold(terminal);
        }
    }

    /// The presentation close owes a failure: every owner it closes, earlier
    /// or later, retains what it withdraws or rejects from now on.
    pub fn preserve(&self) {
        for terminal in &self.terminals {
            terminal.preserve();
        }
    }

    fn hold(&mut self, terminal: CloseTombstone) {
        terminal.enter_close();
        self.terminals.push(terminal);
    }
}

impl Drop for CloseWindow {
    fn drop(&mut self) {
        for terminal in &self.terminals {
            terminal.exit_close();
        }
    }
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

    fn enter_close(&self) {
        self.0.closing.fetch_add(1, Ordering::AcqRel);
    }

    fn exit_close(&self) {
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
