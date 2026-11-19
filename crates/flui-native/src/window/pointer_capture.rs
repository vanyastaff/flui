//! An Activity's pointer capture, the Android lock (ADR-0354 §2): the one
//! owner that reconciles what FLUI's windows want with what the system holds,
//! pure, so every host's tests run it.
//!
//! Capture is the Activity window's, one bit the system keeps
//! (`ViewRootImpl.mPointerCapture`), while FLUI's windows replace one another
//! over the one `FluiView`. The owner holds what the windows want (`wanted`,
//! set by their lock and release commands) and what the system holds
//! (`held`, changed only by the system's answers), and after every command,
//! answer and focus change makes at most the one call (`requestPointerCapture`
//! or `releasePointerCapture`) that moves `held` toward `wanted`, never a
//! second while one is unanswered: from API 26 to 30
//! `ViewRootImpl.requestPointerCapture` returns at once when its last answer
//! already says the asked state, so a call made while another is unanswered
//! is dropped without an answer (`aosp-mirror/platform_frameworks_base`,
//! `android11-release` @ `8551ec36`, `core/java/android/view/ViewRootImpl.java`
//! lines 4551–4556). An answer is attributed to the window the owner asked
//! for, never to "the current one", and the windows' published grabs come
//! from `held` alone.
//!
//! The owner keeps the window's focus as `FluiView` reports it on the Java
//! main thread (`onWindowFocusChanged`, and once as the view enters the
//! window), and makes no call while the window is unfocused: the system
//! drops a request of an unfocused window without an answer. A request made
//! then is kept as wanted, and the focus coming back makes the call it calls
//! for, forgetting any call still unanswered from before. A held capture
//! ends with the focus by the system's `false`, which may come before or
//! after the focus report (`ViewRootImpl` lines 3257 and 3283–3285 send it
//! after; a focus change the input dispatcher reports first sends it
//! before); the owner waits for it in place of a release of its own.
//!
//! A call the system drops without any answer while the window keeps the
//! focus (no such case is known) holds every later command until the next
//! focus change, which forgets it: the owner cannot tell a dropped call from
//! a slow one.

#![forbid(unsafe_code)]

#[cfg(target_os = "android")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "android")]
use flui_platform::CursorGrab;

/// What a lock command does on an Activity window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureRequest {
    /// Ask the owner for the mouse.
    Ask,
    /// Ask nothing: a window without the focus holds no grab, as its
    /// snapshot says.
    Nothing,
    /// Refuse: `FluiView`, the one view that hears the system's answer, is
    /// not in the window.
    Refuse,
}

/// What a window asks for a lock, from whether it has the focus and whether
/// `FluiView` is in it.
pub(crate) const fn capture_request(focused: bool, view_ready: bool) -> CaptureRequest {
    match (focused, view_ready) {
        (false, _) => CaptureRequest::Nothing,
        (true, true) => CaptureRequest::Ask,
        (true, false) => CaptureRequest::Refuse,
    }
}

/// The call the owner makes of the view, for the window `W` it asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureCall<W> {
    /// `View.requestPointerCapture`, for `W`.
    Request(W),
    /// `View.releasePointerCapture`.
    Release,
}

/// What the system holds, as its last answer said, and for whom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held<W> {
    /// No capture.
    Free,
    /// Capture granted to the request the owner made for `W`.
    For(W),
    /// Capture no request of the owner's asked for: it is released.
    Unattributed,
}

impl<W: Copy> Held<W> {
    fn holder(self) -> Option<W> {
        match self {
            Self::For(window) => Some(window),
            Self::Free | Self::Unattributed => None,
        }
    }
}

/// What one command or answer changed: the call to make now, and the windows
/// whose published grab changed, with what each holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureStep<W> {
    /// The one call that moves what is held toward what is wanted.
    pub(crate) call: Option<CaptureCall<W>>,
    /// At most the window that lost the capture and the one that took it.
    pub(crate) changed: [Option<(W, bool)>; 2],
}

/// The Activity's pointer capture (see the module doc).
#[derive(Debug, Clone)]
pub(crate) struct CaptureOwner<W> {
    wanted: Option<W>,
    held: Held<W>,
    /// The call made and not yet answered.
    unanswered: Option<CaptureCall<W>>,
    /// Whether the Activity window has the focus, as `FluiView` last said;
    /// focused until it says, since it says as it enters the window.
    focused: bool,
}

impl<W> Default for CaptureOwner<W> {
    fn default() -> Self {
        Self {
            wanted: None,
            held: Held::Free,
            unanswered: None,
            focused: true,
        }
    }
}

impl<W: Copy + Eq> CaptureOwner<W> {
    /// The window the system holds the capture for: the one whose snapshot
    /// publishes the lock (the steps report each change of it).
    #[cfg(test)]
    pub(crate) fn holder(&self) -> Option<W> {
        self.held.holder()
    }

    /// `window` asks for the mouse.
    pub(crate) fn request(&mut self, window: W) -> CaptureStep<W> {
        self.wanted = Some(window);
        self.settle(self.held)
    }

    /// `window` lets go (its release command, or its retirement); a window
    /// that does not want the mouse changes nothing.
    pub(crate) fn release(&mut self, window: W) -> CaptureStep<W> {
        if self.wanted == Some(window) {
            self.wanted = None;
        }
        self.settle(self.held)
    }

    /// The system answered (`onPointerCaptureChange`): the capture is held or
    /// not. A grant is the window the owner requested for while it still
    /// wants the mouse, and held for no window (published for none, then
    /// released) once it let go meanwhile; a refusal of a request, or a
    /// capture the system ends on its own, ends what the windows wanted too,
    /// so nothing takes the mouse again unasked.
    pub(crate) fn answered(&mut self, has_capture: bool) -> CaptureStep<W> {
        let before = self.held;
        let unanswered = self.unanswered.take();
        self.held = match (has_capture, unanswered) {
            (true, Some(CaptureCall::Request(window))) if self.wanted == Some(window) => {
                Held::For(window)
            }
            (true, _) => match before {
                Held::For(window) => Held::For(window),
                Held::Free | Held::Unattributed => Held::Unattributed,
            },
            (false, Some(CaptureCall::Release)) => Held::Free,
            (false, _) => {
                self.wanted = None;
                Held::Free
            }
        };
        self.settle(before)
    }

    /// `FluiView` said whether the Activity window has the focus. A loss ends
    /// what the windows wanted and forgets an unanswered call, which the
    /// system may drop without an answer; what is held stays the system's to
    /// change, and a held capture is waited for to end by the system's
    /// `false` (before or after this report) in place of a release of the
    /// owner's. A gain forgets any call still unanswered and makes the one
    /// that moves `held` toward `wanted`, a request kept while unfocused
    /// included.
    pub(crate) fn focus_changed(&mut self, focused: bool) -> CaptureStep<W> {
        let before = self.held;
        if self.focused == focused {
            return self.settle(before);
        }
        self.focused = focused;
        self.unanswered = None;
        if !focused {
            self.wanted = None;
            if before != Held::Free {
                self.unanswered = Some(CaptureCall::Release);
            }
        }
        self.settle(before)
    }

    /// `call` never reached the system (the view is gone, or the call
    /// threw): it changed nothing, publishes nothing, and is not made again
    /// until the next command or answer.
    pub(crate) fn call_failed(&mut self, call: CaptureCall<W>) {
        if self.unanswered == Some(call) {
            self.unanswered = None;
        }
    }

    /// The call that moves `held` toward `wanted`, unless one is unanswered
    /// or the window is unfocused, and the publications from `before` to
    /// now.
    fn settle(&mut self, before: Held<W>) -> CaptureStep<W> {
        let call = if self.unanswered.is_some() || !self.focused {
            None
        } else {
            match (self.wanted, self.held) {
                (Some(window), Held::Free) => Some(CaptureCall::Request(window)),
                (Some(window), Held::For(holder)) if holder == window => None,
                (None, Held::Free) => None,
                _ => Some(CaptureCall::Release),
            }
        };
        if call.is_some() {
            self.unanswered = call;
        }
        let (was, now) = (before.holder(), self.held.holder());
        let changed = if was == now {
            [None, None]
        } else {
            [
                was.map(|window| (window, false)),
                now.map(|window| (window, true)),
            ]
        };
        CaptureStep { call, changed }
    }
}

/// A window's copy of whether the owner's `held` is its own, which its
/// snapshot reads on the owner thread.
///
/// Written and read on the owner thread alone. It is atomic only because the
/// loop keeps its window as an `ActivityWindow`, which is `Send + Sync`: the
/// bound is the debt `docs/gaps.md` names under ADR-0327 §1 ("Accepted ADR
/// sections not built yet"), and owner-thread state is a `Cell` by ADR-0323
/// §1 once it goes.
#[cfg(target_os = "android")]
#[derive(Debug, Default)]
pub(crate) struct CaptureMirror {
    held: AtomicBool,
}

#[cfg(target_os = "android")]
impl CaptureMirror {
    /// Record whether the window holds the capture; whether its published
    /// grab changes, so the window publishes.
    #[must_use = "a changed grab is published"]
    pub(crate) fn report(&self, held: bool) -> bool {
        self.held.swap(held, Ordering::AcqRel) != held
    }

    /// The grab the window's snapshot publishes (kept only while it has the
    /// focus, by `WindowState::successor`).
    pub(crate) fn grab(&self) -> CursorGrab {
        if self.held.load(Ordering::Acquire) {
            CursorGrab::Locked
        } else {
            CursorGrab::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureCall, CaptureOwner, CaptureRequest, capture_request};

    const W1: u8 = 1;
    const W2: u8 = 2;

    /// A grant holds the capture for the window asked for, publishing it.
    fn a_grant_holds_the_capture_for_the_window_asked_for() {
        let mut owner = CaptureOwner::default();
        assert_eq!(owner.request(W1).call, Some(CaptureCall::Request(W1)));
        assert_eq!(owner.holder(), None, "nothing held before the answer");
        let granted = owner.answered(true);
        assert_eq!(
            (granted.call, granted.changed),
            (None, [None, Some((W1, true))])
        );
        assert_eq!(owner.holder(), Some(W1));
    }

    /// A release for a replaced window cannot cancel its successor's request:
    /// the successor's request waits for the release's answer, then is made.
    fn a_replaced_windows_release_does_not_cancel_its_successors_request() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        let released = owner.release(W1);
        assert_eq!(released.call, Some(CaptureCall::Release));
        assert_eq!(owner.request(W2).call, None, "the release is unanswered");
        let freed = owner.answered(false);
        assert_eq!(freed.call, Some(CaptureCall::Request(W2)));
        assert_eq!(freed.changed, [Some((W1, false)), None]);
        assert_eq!(owner.answered(true).changed, [None, Some((W2, true))]);
        assert_eq!(owner.holder(), Some(W2));
    }

    /// A release asked while the request is unanswered is not dropped
    /// (API 26–30 ignores it then): the grant is released once it comes, and
    /// nothing is ever published as held.
    fn a_release_before_the_grant_ends_free() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        assert_eq!(owner.release(W1).call, None, "the request is unanswered");
        let granted = owner.answered(true);
        assert_eq!(granted.call, Some(CaptureCall::Release));
        assert_eq!(granted.changed, [None, None], "held for no window");
        assert_eq!(owner.answered(false).changed, [None, None]);
        assert_eq!(owner.holder(), None);
    }

    /// A window that asks while the previous holder's release is unanswered
    /// gets the mouse once the release is answered.
    fn an_ask_during_a_release_gets_the_mouse_after_it() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        owner.release(W1);
        owner.request(W2);
        assert_eq!(owner.answered(false).call, Some(CaptureCall::Request(W2)));
        owner.answered(true);
        assert_eq!(owner.holder(), Some(W2));
    }

    /// A grant that comes after the window that asked was replaced is held
    /// for no window: its successor is not published as holding a capture it
    /// did not ask for, and asks for its own once that one is released.
    fn a_late_grant_for_a_replaced_window_goes_to_no_window() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.release(W1);
        owner.request(W2);
        let late = owner.answered(true);
        assert_eq!(
            (late.call, late.changed),
            (Some(CaptureCall::Release), [None, None])
        );
        assert_eq!(owner.holder(), None);
        assert_eq!(owner.answered(false).call, Some(CaptureCall::Request(W2)));
        assert_eq!(owner.answered(true).changed, [None, Some((W2, true))]);
    }

    /// A call that never reached the system changes nothing: a held capture
    /// stays published as held, the next command is not blocked behind an
    /// answer that never comes, and nothing is made again by itself.
    fn a_failed_call_changes_nothing() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        let release = owner.release(W1).call.expect("a release is made");
        owner.call_failed(release);
        assert_eq!(owner.holder(), Some(W1), "the system still holds it");
        assert_eq!(
            owner.request(W1).call,
            None,
            "wanted and held agree again, nothing to call"
        );
        assert_eq!(
            owner.release(W1).call,
            Some(CaptureCall::Release),
            "the failed release does not stand unanswered"
        );
    }

    /// The focus leaving ends what was wanted: an unanswered request is
    /// forgotten (the system may drop it without an answer, and the next ask
    /// is made all the same), and a late grant is released once the focus is
    /// back.
    fn the_focus_leaving_ends_what_was_wanted() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        assert_eq!(owner.focus_changed(false).call, None);
        assert_eq!(
            owner.focus_changed(true).call,
            None,
            "what was wanted before the loss is not asked for again"
        );
        assert_eq!(
            owner.request(W1).call,
            Some(CaptureCall::Request(W1)),
            "a request the focus loss dropped does not block the next"
        );
        owner.focus_changed(false);
        let late = owner.answered(true);
        assert_eq!(
            (late.call, late.changed),
            (None, [None, None]),
            "a late grant is published for no window, and nothing is called unfocused"
        );
        assert_eq!(
            owner.focus_changed(true).call,
            Some(CaptureCall::Release),
            "it is released once the focus is back"
        );
    }

    /// The focus loss does not end a held capture: the system's `false` that
    /// follows it does. Until then the window publishes the capture the
    /// system holds, no release is made over the system's own, and the next
    /// ask is made once the capture is free, so it never meets API 26–30's
    /// early return for a capture still held.
    fn a_focus_loss_leaves_the_hold_to_the_systems_answer() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        let lost = owner.focus_changed(false);
        assert_eq!((lost.call, lost.changed), (None, [None, None]));
        assert_eq!(owner.holder(), Some(W1), "held until the system says");
        let ended = owner.answered(false);
        assert_eq!(
            (ended.call, ended.changed),
            (None, [Some((W1, false)), None])
        );
        assert_eq!(owner.focus_changed(true).call, None);
        assert_eq!(owner.request(W1).call, Some(CaptureCall::Request(W1)));
        assert_eq!(owner.answered(true).changed, [None, Some((W1, true))]);
    }

    /// The system's `false` may come before the focus report: it ends the
    /// hold, the report after it changes nothing more, and the next ask is
    /// made once the focus is back.
    fn the_systems_false_before_the_focus_report_ends_the_hold() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        let ended = owner.answered(false);
        assert_eq!(
            (ended.call, ended.changed),
            (None, [Some((W1, false)), None])
        );
        let lost = owner.focus_changed(false);
        assert_eq!((lost.call, lost.changed), (None, [None, None]));
        assert_eq!(owner.focus_changed(true).call, None);
        assert_eq!(owner.request(W1).call, Some(CaptureCall::Request(W1)));
    }

    /// A window that asks after the Activity window lost the focus, before
    /// the owner thread heard of it, gets no call the system would drop
    /// unanswered: the request is kept and made once the focus is back.
    fn a_request_while_unfocused_is_made_when_the_focus_returns() {
        let mut owner = CaptureOwner::default();
        owner.focus_changed(false);
        assert_eq!(owner.request(W1).call, None, "no call while unfocused");
        assert_eq!(
            owner.focus_changed(true).call,
            Some(CaptureCall::Request(W1))
        );
        assert_eq!(owner.answered(true).changed, [None, Some((W1, true))]);
    }

    /// After the focus is back, a release and a new ask each make their
    /// call: nothing from the unfocused time stands unanswered.
    fn a_release_and_an_ask_after_the_focus_returns_are_made() {
        let mut owner = CaptureOwner::default();
        owner.focus_changed(false);
        owner.request(W1);
        owner.focus_changed(true);
        owner.answered(true);
        assert_eq!(owner.release(W1).call, Some(CaptureCall::Release));
        owner.answered(false);
        assert_eq!(owner.request(W2).call, Some(CaptureCall::Request(W2)));
    }

    /// A call still unanswered when the focus comes back (the system's
    /// `false` for a capture held at the loss has not come) is forgotten,
    /// and the call that moves the capture toward what is wanted is made.
    fn a_call_unanswered_across_the_focus_return_is_forgotten() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        owner.focus_changed(false);
        assert_eq!(
            owner.focus_changed(true).call,
            Some(CaptureCall::Release),
            "the capture held for no wanted window is released"
        );
    }

    /// A capture the system ends on its own (another window took the focus
    /// at once) is not asked for again.
    fn a_capture_the_system_ends_is_not_asked_for_again() {
        let mut owner = CaptureOwner::default();
        owner.request(W1);
        owner.answered(true);
        let ended = owner.answered(false);
        assert_eq!(
            (ended.call, ended.changed),
            (None, [Some((W1, false)), None])
        );
    }

    /// Only a focused window with its view asks; an unfocused one asks
    /// nothing, and one without `FluiView` refuses.
    fn only_a_focused_window_with_its_view_asks_for_the_mouse() {
        assert_eq!(capture_request(true, true), CaptureRequest::Ask);
        assert_eq!(capture_request(false, true), CaptureRequest::Nothing);
        assert_eq!(capture_request(false, false), CaptureRequest::Nothing);
        assert_eq!(capture_request(true, false), CaptureRequest::Refuse);
    }

    #[test]
    fn android_pointer_capture() {
        crate::table_test::run_table(
            "android_pointer_capture",
            &[
                (
                    "a_grant_holds_the_capture_for_the_window_asked_for",
                    a_grant_holds_the_capture_for_the_window_asked_for,
                ),
                (
                    "a_replaced_windows_release_does_not_cancel_its_successors_request",
                    a_replaced_windows_release_does_not_cancel_its_successors_request,
                ),
                (
                    "a_release_before_the_grant_ends_free",
                    a_release_before_the_grant_ends_free,
                ),
                (
                    "an_ask_during_a_release_gets_the_mouse_after_it",
                    an_ask_during_a_release_gets_the_mouse_after_it,
                ),
                (
                    "a_late_grant_for_a_replaced_window_goes_to_no_window",
                    a_late_grant_for_a_replaced_window_goes_to_no_window,
                ),
                (
                    "a_failed_call_changes_nothing",
                    a_failed_call_changes_nothing,
                ),
                (
                    "the_focus_leaving_ends_what_was_wanted",
                    the_focus_leaving_ends_what_was_wanted,
                ),
                (
                    "a_focus_loss_leaves_the_hold_to_the_systems_answer",
                    a_focus_loss_leaves_the_hold_to_the_systems_answer,
                ),
                (
                    "the_systems_false_before_the_focus_report_ends_the_hold",
                    the_systems_false_before_the_focus_report_ends_the_hold,
                ),
                (
                    "a_request_while_unfocused_is_made_when_the_focus_returns",
                    a_request_while_unfocused_is_made_when_the_focus_returns,
                ),
                (
                    "a_release_and_an_ask_after_the_focus_returns_are_made",
                    a_release_and_an_ask_after_the_focus_returns_are_made,
                ),
                (
                    "a_call_unanswered_across_the_focus_return_is_forgotten",
                    a_call_unanswered_across_the_focus_return_is_forgotten,
                ),
                (
                    "a_capture_the_system_ends_is_not_asked_for_again",
                    a_capture_the_system_ends_is_not_asked_for_again,
                ),
                (
                    "only_a_focused_window_with_its_view_asks_for_the_mouse",
                    only_a_focused_window_with_its_view_asks_for_the_mouse,
                ),
            ],
        );
    }
}
