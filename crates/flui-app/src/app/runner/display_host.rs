//! The host's accepted display snapshot, and the windows that follow it
//! (ADR-0236).
//!
//! The platform samples; the host commits the snapshot it accepted *before*
//! anyone is told, then derives, for every live window, which display that
//! window is on and what that display's refresh period is, and delivers the
//! result to the window's presentation as an ordinary window observation. The
//! presentation compares it with the value it holds and acts only on a real
//! difference, so the host may deliver its derivation after every sample
//! without waking windows whose display did not change. A window whose
//! delivery failed part-way keeps the previous value and receives the same
//! derivation again on the next sample.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use flui_platform::traits::PlatformWindow;
use flui_platform_api::DisplaySnapshot;
use flui_runtime::owner::DisplayUpdate;

use super::owner_dispatch::PresentationDispatcher;

/// A window whose display the host keeps current.
struct FollowedWindow {
    dispatcher: PresentationDispatcher,
    /// Weak: the window's own callback table holds the hint that reaches the
    /// host, so a strong reference here would keep a closed window alive.
    window: Weak<dyn PlatformWindow>,
}

#[derive(Default)]
struct DisplayState {
    accepted: Option<DisplaySnapshot>,
    windows: Vec<FollowedWindow>,
    /// The message of the last failed sample, so a source that fails on every
    /// owner turn is reported once, not once per turn.
    last_failure: Option<String>,
}

/// Accepted display state of one installed host incarnation.
#[derive(Default)]
pub(super) struct DisplayHost {
    state: RefCell<DisplayState>,
}

impl DisplayHost {
    /// Commit `snapshot` as the host's view of the displays. Nothing is
    /// delivered here: the caller notifies windows after this returns.
    pub(super) fn accept(&self, snapshot: DisplaySnapshot) {
        let mut state = self.state.borrow_mut();
        state.accepted = Some(snapshot);
        state.last_failure = None;
    }

    /// The snapshot the host last accepted.
    pub(super) fn snapshot(&self) -> Option<DisplaySnapshot> {
        self.state.borrow().accepted.clone()
    }

    /// Record a failed sample. Returns whether this failure differs from the
    /// previous one and so deserves a diagnostic; the accepted snapshot stays.
    pub(super) fn note_failure(&self, message: String) -> bool {
        let mut state = self.state.borrow_mut();
        let new = state.last_failure.as_deref() != Some(message.as_str());
        state.last_failure = Some(message);
        new
    }

    /// Follow `window` from now on.
    pub(super) fn follow(
        &self,
        dispatcher: PresentationDispatcher,
        window: &Arc<dyn PlatformWindow>,
    ) {
        self.state.borrow_mut().windows.push(FollowedWindow {
            dispatcher,
            window: Arc::downgrade(window),
        });
    }

    /// Stop following the window of `address`.
    pub(super) fn unfollow(&self, address: flui_foundation::PresentationAddress) {
        self.state
            .borrow_mut()
            .windows
            .retain(|followed| followed.dispatcher.address != address);
    }

    /// What `window` is on according to the accepted snapshot; unknown before
    /// the first sample, for a window naming no display, and for one the
    /// snapshot does not hold.
    pub(super) fn derive(&self, window: &dyn PlatformWindow) -> DisplayUpdate {
        let snapshot = self.snapshot();
        // The window answers outside any host borrow: it is platform code.
        let id = window.display_id();
        DisplayUpdate::new(snapshot.and_then(|snapshot| snapshot.window_display(id)))
    }

    /// The derived display of every live followed window. Windows that are gone
    /// are forgotten.
    pub(super) fn updates(&self) -> Vec<(PresentationDispatcher, DisplayUpdate)> {
        let live: Vec<(PresentationDispatcher, Arc<dyn PlatformWindow>)> = {
            let mut state = self.state.borrow_mut();
            state
                .windows
                .retain(|followed| followed.window.strong_count() > 0);
            state
                .windows
                .iter()
                .filter_map(|followed| Some((followed.dispatcher, followed.window.upgrade()?)))
                .collect()
        };
        live.into_iter()
            .map(|(dispatcher, window)| (dispatcher, self.derive(window.as_ref())))
            .collect()
    }
}
