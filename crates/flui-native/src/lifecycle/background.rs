//! The background window of a phone: once the application leaves the screen
//! the system may suspend or end its process, so the host tells the
//! application its session is ending and keeps the process running while the
//! application's writes finish, within a bound.
//!
//! [`BackgroundWindow`] is the pure part: when a trip to the background is
//! announced, and which of release, expiry and a return to the foreground
//! ends the platform's task. Each task ends exactly once. UIKit's background
//! task is the one platform object it holds today; Android keeps a stopped
//! process running until the system reclaims it, so its host holds nothing
//! and the bound below is what the application waits for there.

use std::time::Duration;

/// How long an iOS application's writes may run after its scene went to the
/// background: UIKit grants a background task about thirty seconds, and the
/// application is told less so it reports before the system ends the task.
#[cfg(target_os = "ios")]
pub(crate) const IOS_BACKGROUND_WINDOW: Duration = Duration::from_secs(20);

/// How long an Android application's writes may run after its Activity
/// stopped: the process keeps running after `onStop`, until the system
/// reclaims it or the cached-app freezer stops it, ten seconds later by
/// default; the application is told half of that.
#[cfg(target_os = "android")]
pub(crate) const ANDROID_BACKGROUND_WINDOW: Duration = Duration::from_secs(5);

/// One trip to the background and the platform task `T` held for it.
#[derive(Debug)]
#[cfg_attr(
    target_os = "android",
    expect(dead_code, reason = "only UIKit holds a background task")
)]
pub(crate) enum BackgroundWindow<T> {
    /// On screen, or never shown: the next trip to the background is
    /// announced.
    Foreground,
    /// In the background, announced, with no task held.
    Announced,
    /// In the background, with the task the application's writes hold.
    Holding(T),
}

#[cfg_attr(
    target_os = "android",
    expect(dead_code, reason = "only UIKit holds a background task")
)]
impl<T> BackgroundWindow<T> {
    /// The application left the screen: `true` for the first time in this
    /// trip, which the host announces as the end of the session. A second
    /// report of the same trip announces nothing.
    pub(crate) fn entered_background(&mut self) -> bool {
        if matches!(self, Self::Foreground) {
            *self = Self::Announced;
            true
        } else {
            false
        }
    }

    /// The application answered the announcement by holding the session for
    /// its writes, and the platform gave `task` for it. A task offered with
    /// none to hold for (back on screen meanwhile, or held already) is handed
    /// back for the host to end at once.
    #[must_use = "a task handed back must be ended"]
    pub(crate) fn hold(&mut self, task: T) -> Option<T> {
        if matches!(self, Self::Announced) {
            *self = Self::Holding(task);
            None
        } else {
            Some(task)
        }
    }

    /// The application released its hold: the task to end, if one is held.
    #[must_use = "the task returned must be ended"]
    pub(crate) fn release(&mut self) -> Option<T> {
        self.end(Self::Announced)
    }

    /// The system ended the window before the application released it: the
    /// task to end, if one is still held. Expiry after a release ends
    /// nothing.
    #[must_use = "the task returned must be ended"]
    pub(crate) fn expired(&mut self) -> Option<T> {
        self.end(Self::Announced)
    }

    /// The application came back on screen: the next trip is announced again,
    /// and a task still held is ended, since the foreground needs none.
    #[must_use = "the task returned must be ended"]
    pub(crate) fn entering_foreground(&mut self) -> Option<T> {
        self.end(Self::Foreground)
    }

    /// Move to `next`, handing back the task held, if one is. A window on
    /// screen stays there: a late release or expiry announces nothing.
    fn end(&mut self, next: Self) -> Option<T> {
        if matches!(self, Self::Foreground) {
            return None;
        }
        match std::mem::replace(self, next) {
            Self::Holding(task) => Some(task),
            Self::Announced | Self::Foreground => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BackgroundWindow;

    /// The task is taken in the background and ended exactly once, by
    /// whichever of release, expiry and a return to the foreground comes
    /// first; each trip is announced once.
    ///
    /// Red-check: let a return to the foreground leave the trip announced —
    /// no row announces its next trip.
    #[test]
    fn background_window_ends_each_task_exactly_once() {
        type Step = fn(&mut BackgroundWindow<u32>) -> Option<u32>;
        let release: Step = BackgroundWindow::release;
        let expire: Step = BackgroundWindow::expired;
        let foreground: Step = BackgroundWindow::entering_foreground;
        for (case, steps, ended) in [
            (
                "released",
                vec![release, expire, foreground],
                vec![Some(1), None, None],
            ),
            (
                "expired",
                vec![expire, release, foreground],
                vec![Some(1), None, None],
            ),
            (
                "back on screen",
                vec![foreground, release, expire],
                vec![Some(1), None, None],
            ),
        ] {
            let mut window = BackgroundWindow::Foreground;
            assert!(window.entered_background(), "{case}: the trip is announced");
            assert!(!window.entered_background(), "{case}: once");
            assert_eq!(window.hold(1), None, "{case}: the task is held");
            let got: Vec<_> = steps.into_iter().map(|step| step(&mut window)).collect();
            assert_eq!(got, ended, "{case}");
            assert!(
                window.entered_background(),
                "{case}: the next trip is announced again"
            );
        }
    }

    /// A task offered when nothing waits for it is handed back to be ended:
    /// on screen, or with a task held already.
    #[test]
    fn background_window_hands_back_a_task_it_cannot_hold() {
        let mut window = BackgroundWindow::Foreground;
        assert_eq!(window.hold(1), Some(1), "on screen");
        assert!(window.entered_background());
        assert_eq!(window.hold(2), None);
        assert_eq!(window.hold(3), Some(3), "a task is held already");
        assert_eq!(window.release(), Some(2));
        assert_eq!(window.release(), None, "released already");
        assert!(matches!(window, BackgroundWindow::Announced));
        assert_eq!(window.entering_foreground(), None);
        assert!(matches!(window, BackgroundWindow::Foreground));
        assert_eq!(window.release(), None);
        assert!(
            matches!(window, BackgroundWindow::Foreground),
            "a release on screen leaves the window on screen"
        );
    }
}
