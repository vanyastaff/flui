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
}

/// The table of the one visibility rule: a window that reports a run of
/// answers dispatches each change once, from whichever answer it opened as.
#[cfg(test)]
mod mapping {
    use super::ReportedVisibility;

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

    #[test]
    fn every_change_is_reported_once() {
        for (case, opened, run) in ROWS {
            let mut reported = ReportedVisibility::new(opened);
            for (step, (answer, owed)) in run.iter().enumerate() {
                assert_eq!(reported.report(*answer), *owed, "{case}, answer {step}");
                assert_eq!(
                    reported,
                    ReportedVisibility::new(*answer),
                    "{case}, answer {step}: the runtime holds the answer"
                );
            }
        }
    }
}
