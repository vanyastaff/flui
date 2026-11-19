//! An Activity window's pointer capture, the Android lock (ADR-0354 §2): the
//! pure rules `AndroidWindow` and the loop follow, apart from the JNI calls,
//! so every host's tests run them.
//!
//! The window asks for the mouse only while it has the focus and `FluiView`
//! is in the window, and publishes the system's answer
//! (`onPointerCaptureChange`) as its grab: the snapshot says what the system
//! holds, never what was asked. The view serves one window at a time, but an
//! answer can come after the window that asked was replaced: it is matched to
//! the window that asked ([`route_answer`]), never to "the current one".

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use flui_platform::CursorGrab;

/// What a lock command does on an Activity window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureRequest {
    /// Ask the system for the mouse (`View.requestPointerCapture`).
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

/// Where the system's answer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureAnswer {
    /// To the window that asked, which is the loop's window.
    Deliver,
    /// Nowhere, and the capture is given back: the system holds the mouse for
    /// a window that is gone (or for no window that asked).
    Release,
    /// Nowhere: the system let go of a mouse no current window holds.
    Ignore,
}

/// Where an answer (`held`) goes, given the window that last asked for the
/// mouse when the view made the request (`asker`, recorded on the thread the
/// answers come on, so an answer that comes after a newer request is the
/// newer request's) and the loop's window now (`current`).
pub(crate) fn route_answer<W: Eq>(
    asker: Option<W>,
    current: Option<W>,
    held: bool,
) -> CaptureAnswer {
    match (asker, current) {
        (Some(asker), Some(current)) if asker == current => CaptureAnswer::Deliver,
        _ if held => CaptureAnswer::Release,
        _ => CaptureAnswer::Ignore,
    }
}

/// The system's last word on whether the window has the mouse, and what the
/// window publishes on each exit that changes it.
///
/// Written and read on the owner thread alone. It is atomic only because the
/// loop keeps its window as an `ActivityWindow`, which is `Send + Sync`: the
/// bound is the debt `docs/gaps.md` names under ADR-0327 §1 ("Accepted ADR
/// sections not built yet"), and owner-thread state is a `Cell` by ADR-0323
/// §1 once it goes.
#[derive(Debug, Default)]
pub(crate) struct CaptureMirror {
    held: AtomicBool,
}

impl CaptureMirror {
    /// Record the system's answer, or a release that never reached the view
    /// (the view is not in the window: no capture is held); whether the
    /// published grab changes, so the window publishes.
    #[must_use = "a changed grab is published"]
    pub(crate) fn report(&self, held: bool) -> bool {
        self.held.swap(held, Ordering::AcqRel) != held
    }

    /// Whether the release command's exit publishes: it does, whatever is
    /// held meanwhile, as the release is the system's to answer (ADR-0354
    /// §2: every exit of a grab command publishes).
    #[must_use = "the release command's exit publishes"]
    pub(crate) const fn release_publishes() -> bool {
        true
    }

    /// The window is retired: it gives the mouse back, and holds nothing from
    /// now on; whether the published grab changes, so it publishes.
    #[must_use = "a changed grab is published"]
    pub(crate) fn retire(&self) -> bool {
        self.report(false)
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
    use flui_platform::CursorGrab;

    use super::{CaptureAnswer, CaptureMirror, CaptureRequest, capture_request, route_answer};

    /// The system's answer is the published grab: a capture it gives is
    /// `Locked` and changes the snapshot once, a capture it takes back is
    /// `None`.
    fn the_systems_capture_is_the_published_grab() {
        let mirror = CaptureMirror::default();
        assert_eq!(mirror.grab(), CursorGrab::None);
        assert!(mirror.report(true), "a capture given changes the grab");
        assert_eq!(mirror.grab(), CursorGrab::Locked);
        assert!(
            !mirror.report(true),
            "the same answer again changes nothing"
        );
        assert!(
            mirror.report(false),
            "a capture taken back changes the grab"
        );
        assert_eq!(mirror.grab(), CursorGrab::None);
    }

    /// Every exit that changes the grab publishes: the release command's,
    /// a release the view never got (reported as no capture), and the
    /// window's retirement.
    fn every_exit_that_changes_the_grab_publishes() {
        let mirror = CaptureMirror::default();
        assert!(mirror.report(true));
        assert!(
            CaptureMirror::release_publishes(),
            "the release command publishes"
        );

        assert!(
            mirror.report(false),
            "an undelivered release publishes none held"
        );
        assert_eq!(mirror.grab(), CursorGrab::None);

        assert!(mirror.report(true));
        assert!(mirror.retire(), "retirement publishes the grab let go");
        assert_eq!(mirror.grab(), CursorGrab::None);
        assert!(
            !mirror.retire(),
            "a window that holds nothing changes nothing"
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

    /// An answer goes to the window that asked: a late grant for a window
    /// replaced since gives the mouse back instead of locking its successor,
    /// and a late release for it changes nothing.
    fn an_answer_goes_to_the_window_that_asked() {
        const W1: u8 = 1;
        const W2: u8 = 2;
        assert_eq!(
            route_answer(Some(W2), Some(W2), true),
            CaptureAnswer::Deliver
        );
        assert_eq!(
            route_answer(Some(W2), Some(W2), false),
            CaptureAnswer::Deliver
        );
        assert_eq!(
            route_answer(Some(W1), Some(W2), true),
            CaptureAnswer::Release
        );
        assert_eq!(
            route_answer(Some(W1), Some(W2), false),
            CaptureAnswer::Ignore
        );
        assert_eq!(route_answer(Some(W1), None, true), CaptureAnswer::Release);
        assert_eq!(route_answer(None, Some(W2), true), CaptureAnswer::Release);
        assert_eq!(route_answer::<u8>(None, None, false), CaptureAnswer::Ignore);
    }

    #[test]
    fn android_pointer_capture() {
        crate::table_test::run_table(
            "android_pointer_capture",
            &[
                (
                    "the_systems_capture_is_the_published_grab",
                    the_systems_capture_is_the_published_grab,
                ),
                (
                    "every_exit_that_changes_the_grab_publishes",
                    every_exit_that_changes_the_grab_publishes,
                ),
                (
                    "only_a_focused_window_with_its_view_asks_for_the_mouse",
                    only_a_focused_window_with_its_view_asks_for_the_mouse,
                ),
                (
                    "an_answer_goes_to_the_window_that_asked",
                    an_answer_goes_to_the_window_that_asked,
                ),
            ],
        );
    }
}
