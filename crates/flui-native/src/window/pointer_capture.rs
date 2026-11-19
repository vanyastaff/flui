//! An Activity window's pointer capture, the Android lock (ADR-0354 §2): the
//! pure rules `AndroidWindow` follows, apart from the JNI calls, so every
//! host's tests run them.
//!
//! The window asks for the mouse only while it has the focus and `FluiView`
//! is in the window, and publishes the system's answer
//! (`onPointerCaptureChange`) as its grab: the snapshot says what the system
//! holds, never what was asked.

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

/// The system's last word on whether the window has the mouse.
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
    /// Record the system's answer; whether the published grab changes, so
    /// the window publishes.
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
    use flui_platform::CursorGrab;

    use super::{CaptureMirror, CaptureRequest, capture_request};

    /// The system's answer is the published grab: a capture it gives is
    /// `Locked` and changes the snapshot once, a capture it takes back is
    /// `None`.
    #[test]
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

    /// Only a focused window with its view asks; an unfocused one asks
    /// nothing, and one without `FluiView` refuses.
    #[test]
    fn only_a_focused_window_with_its_view_asks_for_the_mouse() {
        assert_eq!(capture_request(true, true), CaptureRequest::Ask);
        assert_eq!(capture_request(false, true), CaptureRequest::Nothing);
        assert_eq!(capture_request(false, false), CaptureRequest::Nothing);
        assert_eq!(capture_request(true, false), CaptureRequest::Refuse);
    }
}
