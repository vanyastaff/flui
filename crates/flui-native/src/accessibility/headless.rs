//! The headless host's accessibility: a recorder of what a window published.

use parking_lot::Mutex;

/// Recording fake for [`PlatformAccessibility`](flui_semantics::platform::PlatformAccessibility),
/// backing the headless backend's
/// [`HostWindow::accessibility`](crate::HostWindow::accessibility).
///
/// Records every published tree so a test can assert what an assistive
/// technology would actually have been told — the roles, labels and ids — not
/// merely that publishing did not panic. Activation is drivable
/// ([`set_active`](Self::set_active)) because the interesting behaviour is
/// conditional on it: a composition root must start assembly on attach and stop
/// on detach, and neither is observable without being able to fake the signal.
#[derive(Default)]
pub struct FakeAccessibility {
    state: Mutex<FakeAccessibilityState>,
}

#[derive(Default)]
struct FakeAccessibilityState {
    active: bool,
    published: Vec<accesskit::TreeUpdate>,
    activation_listener: Option<flui_semantics::platform::AccessibilityActivationListener>,
    action_listener: Option<flui_semantics::platform::AccessibilityActionListener>,
}

impl FakeAccessibility {
    /// Create a fresh recorder, inactive and with nothing published.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every tree published while active, in order.
    #[must_use]
    pub fn published(&self) -> Vec<accesskit::TreeUpdate> {
        self.state.lock().published.clone()
    }

    /// How many trees were published while active.
    #[must_use]
    pub fn published_count(&self) -> usize {
        self.state.lock().published.len()
    }

    /// Simulate assistive technology attaching or detaching, notifying the
    /// registered listener.
    ///
    /// The listener is cloned out of the lock before being called, so a
    /// listener that publishes (or otherwise re-enters this fake) does not
    /// deadlock against the guard that dispatched it — the same
    /// clone-and-release discipline the real platform paths use.
    pub fn set_active(&self, active: bool) {
        let listener = {
            let mut state = self.state.lock();
            // Attach/detach is a *transition*. Re-notifying on an unchanged
            // state would drive redundant enable/disable cycles in a
            // composition root that starts and stops assembly off this signal.
            if state.active == active {
                return;
            }
            state.active = active;
            state.activation_listener.clone()
        };
        if let Some(listener) = listener {
            listener(active);
        }
    }

    /// Simulate assistive technology requesting an action.
    ///
    /// Dropped while inactive, because that is what the platform does: actions
    /// originate from an attached client, and one arriving after detach is
    /// stale. Forwarding it anyway would let a higher layer depend on
    /// action delivery outside an active session and pass here while failing
    /// against a real adapter.
    pub fn request_action(&self, request: accesskit::ActionRequest) {
        let listener = {
            let state = self.state.lock();
            if !state.active {
                return;
            }
            state.action_listener.clone()
        };
        if let Some(listener) = listener {
            listener(request);
        }
    }
}

impl std::fmt::Debug for FakeAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("FakeAccessibility")
            .field("active", &state.active)
            .field("published_count", &state.published.len())
            .finish_non_exhaustive()
    }
}

impl flui_semantics::platform::PlatformAccessibility for FakeAccessibility {
    fn publish(&self, update: accesskit::TreeUpdate) {
        let mut state = self.state.lock();
        // Inactive means nothing is listening: the update is dropped rather
        // than recorded, mirroring the real adapter's `update_if_active`.
        if state.active {
            state.published.push(update);
        }
    }

    fn is_active(&self) -> bool {
        self.state.lock().active
    }

    fn set_activation_listener(
        &self,
        listener: flui_semantics::platform::AccessibilityActivationListener,
    ) {
        self.state.lock().activation_listener = Some(listener);
    }

    fn set_action_listener(&self, listener: flui_semantics::platform::AccessibilityActionListener) {
        self.state.lock().action_listener = Some(listener);
    }
}
