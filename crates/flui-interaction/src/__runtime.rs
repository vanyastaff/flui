//! Presentation-owner terminal cleanup. No application API or semver promise.

pub use crate::close::CloseMode;
use crate::close::CloseTombstone;
pub use crate::pointer::interaction_lane::DispatchCustody;
use crate::{FocusManager, TextInputOwner};

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

/// Mark `node` as created by a widget for itself: once that widget detaches
/// it (on dispose) or replaces it, a request on it answers
/// [`FocusRequestOutcome::Stale`](crate::FocusRequestOutcome::Stale) instead
/// of waiting for an attachment that will never come.
pub fn mark_widget_owned(node: &crate::FocusNode) {
    node.mark_widget_owned();
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

/// Sample gesture deadlines under the presentation's failure custody.
pub fn tick_deadlines_with_recovery(
    owner: &crate::GestureBinding,
    now: web_time::Instant,
    recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
) {
    owner.deadlines_with_recovery(now, true, recovery);
}

/// Refresh scheduling observation during a guard-free owner operation.
pub fn refresh_deadlines_with_recovery(
    owner: &crate::GestureBinding,
    now: web_time::Instant,
    recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
) {
    owner.deadlines_with_recovery(now, false, recovery);
}

/// Read the last owner scheduling observation without executing member code.
#[must_use]
pub fn next_deadline_snapshot(
    owner: &crate::GestureBinding,
) -> Option<flui_platform_api::EventTime> {
    owner.arena().next_deadline_snapshot()
}

/// The instant a deadline on `owner`'s event timeline names on its clock, for
/// the host's native wake; `None` when it lies past the last instant the
/// platform represents, a deadline no wake can reach.
#[must_use]
pub fn wake_instant(
    owner: &crate::GestureBinding,
    deadline: flui_platform_api::EventTime,
) -> Option<web_time::Instant> {
    owner.instant_at(deadline)
}

/// Read ordinary deadline demand, excluding quarantined discovery and polling.
#[must_use]
pub fn has_pending_deadlines_snapshot(owner: &crate::GestureBinding) -> bool {
    owner.arena().has_pending_deadlines_snapshot()
}

/// Withdraw mouse publication and callback ownership for a terminal presentation.
pub fn close_mouse_tracker(owner: &crate::pointer::MouseTracker, mode: CloseMode) {
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

    /// Hold a presentation dispatch owner's window, or a raw handle's lane window.
    pub fn dispatch(&mut self, handle: &crate::InteractionDispatchHandle) {
        self.hold(handle.close_tombstone());
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
