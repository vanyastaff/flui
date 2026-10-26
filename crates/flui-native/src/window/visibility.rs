//! The one rule by which a window tells the runtime whether its surface is
//! seen.
//!
//! The runtime's hidden-surface gating (`PlatformWindow::
//! on_visibility_status_change` → `PlatformToUi::WindowVisibility` → the
//! per-presentation `FrameClock` gate and the UI runtime-wide
//! `AppLifecycleState` derivation) only ever fires if a backend actually
//! *dispatches* the callback. Each backend reads what decides the answer
//! from its own OS (winit's `Occluded` beside the window's shown and
//! minimized states, the Win32 window's messages with a cloak and a locked
//! session, AppKit's occlusion state, the page's `visibilitychange`), and
//! every one reports it through [`ReportedVisibility`]: the runtime hears of
//! the answer when it changes, never again for an answer it holds, however
//! often or late the OS repeats it.
//!
//! Contract note (mirrors `PlatformWindow::on_visibility_status_change`):
//! the callback parameter is `is_visible` — `true` when the window's surface
//! became visible again, `false` when it stopped being visible (minimized,
//! hidden, or fully occluded).

#![forbid(unsafe_code)]

/// What the runtime was last told of a window's visibility, and the one rule
/// for telling it again: a report is owed only when the answer differs from
/// the last one.
///
/// A backend keeps one per window, starting from what the window opened as,
/// and asks [`report`](Self::report) with the answer now; it dispatches what
/// that returns once its locks are released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReportedVisibility {
    last: bool,
}

impl ReportedVisibility {
    /// The runtime knows the window as `visible`.
    pub(crate) const fn new(visible: bool) -> Self {
        Self { last: visible }
    }

    /// The report a window that is now `visible` owes: `Some(visible)` when
    /// it differs from the last one, which it becomes; `None` when the runtime
    /// already holds it.
    #[must_use]
    pub(crate) fn report(&mut self, visible: bool) -> Option<bool> {
        (std::mem::replace(&mut self.last, visible) != visible).then_some(visible)
    }

    /// [`report`](Self::report) for a window that keeps what the runtime
    /// was told in a `Cell` (its owner-thread state: the Win32 and Web
    /// windows).
    #[cfg(any(windows, target_arch = "wasm32", test))]
    #[must_use]
    pub(crate) fn report_in(reported: &std::cell::Cell<Self>, visible: bool) -> Option<bool> {
        let mut last = reported.get();
        let report = last.report(visible);
        reported.set(last);
        report
    }
}

/// The table of the one visibility rule: a window that reads a run of
/// answers reports each change once, from whichever answer it opened as.
/// The Win32 and AppKit windows read their answer from their messages and
/// occlusion state with their pure readers, which the table drives; the
/// winit and Web windows read theirs through FFI and report it as read.
#[cfg(test)]
mod mapping {
    use std::cell::Cell;

    use super::ReportedVisibility;
    use crate::window::apple::geometry::surface::{
        OCCLUSION_STATE_VISIBLE, occlusion_state_is_visible,
    };
    use crate::window::windows::surface::{
        SIZE_MINIMIZED, SIZE_RESTORED, show_window_visibility, size_visibility,
    };

    /// The answers a window reads, in order, each with the report it owes.
    type Run = &'static [(bool, Option<bool>)];

    /// (case, the answer the window opened as, what follows)
    const ROWS: [(&str, bool, Run); 6] = [
        ("an unchanged visible window", true, &[(true, None)]),
        ("an unchanged hidden window", false, &[(false, None)]),
        (
            "hidden, then shown",
            true,
            &[(false, Some(false)), (true, Some(true))],
        ),
        (
            "a repeated or late answer",
            true,
            &[
                (false, Some(false)),
                (false, None),
                (true, Some(true)),
                (true, None),
            ],
        ),
        (
            "a window that opened hidden",
            false,
            &[(true, Some(true)), (false, Some(false))],
        ),
        (
            "the same answer from two sources",
            true,
            &[(false, Some(false)), (false, None), (false, None)],
        ),
    ];

    /// AppKit's `occlusionState` for the answer: the visible bit, or none.
    const fn occlusion(visible: bool) -> u64 {
        if visible { OCCLUSION_STATE_VISIBLE } else { 0 }
    }

    #[test]
    fn every_change_is_reported_once() {
        for (case, opened, run) in ROWS {
            let mut read_through_ffi = ReportedVisibility::new(opened);
            let web = Cell::new(ReportedVisibility::new(opened));
            let mut appkit = ReportedVisibility::new(opened);
            // A Win32 window hidden by `ShowWindow`, and one minimized and
            // restored, which opened minimized when it opened hidden.
            let shown = Cell::new(ReportedVisibility::new(opened));
            let sized = Cell::new(ReportedVisibility::new(opened));
            let mut minimized = !opened;
            for (step, (answer, owed)) in run.iter().copied().enumerate() {
                let size_type = if answer {
                    SIZE_RESTORED
                } else {
                    SIZE_MINIMIZED
                };
                let size_edge = size_visibility(size_type, minimized);
                minimized = !answer;
                let reports = [
                    ("winit", read_through_ffi.report(answer)),
                    ("Web", ReportedVisibility::report_in(&web, answer)),
                    (
                        "AppKit",
                        appkit.report(occlusion_state_is_visible(occlusion(answer))),
                    ),
                    (
                        "Win32 WM_SHOWWINDOW",
                        show_window_visibility(answer, false)
                            .and_then(|visible| ReportedVisibility::report_in(&shown, visible)),
                    ),
                    (
                        "Win32 WM_SIZE",
                        size_edge
                            .and_then(|visible| ReportedVisibility::report_in(&sized, visible)),
                    ),
                ];
                for (system, report) in reports {
                    assert_eq!(report, owed, "{system}: {case}, answer {step}");
                }
            }
        }
    }
}
