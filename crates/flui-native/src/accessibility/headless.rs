//! The headless host's accessibility: a counter of what a window published.

use parking_lot::Mutex;

/// The headless host's [`PlatformAccessibility`](flui_semantics::platform::PlatformAccessibility),
/// backing its windows'
/// [`HostWindow::accessibility`](crate::HostWindow::accessibility).
///
/// Counts the trees published while an assistive technology is attached, so a
/// test can tell that publishing reached the platform only then. Activation is
/// drivable ([`set_active`](Self::set_active)) because the interesting
/// behaviour is conditional on it: a composition root must start assembly on
/// attach and stop on detach, and neither is observable without being able to
/// fake the signal. Its own methods name no AccessKit type (ADR-0151 §8): the
/// trees and action listeners it hears come through `PlatformAccessibility`,
/// `flui-semantics`' trait.
#[derive(Default)]
pub struct FakeAccessibility {
    state: Mutex<FakeAccessibilityState>,
}

#[derive(Default)]
struct FakeAccessibilityState {
    active: bool,
    published: usize,
    activation_listener: Option<flui_semantics::platform::AccessibilityActivationListener>,
}

impl FakeAccessibility {
    /// Create a fresh recorder, inactive and with nothing published.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many trees were published while active.
    #[must_use]
    pub fn published_count(&self) -> usize {
        self.state.lock().published
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
}

impl std::fmt::Debug for FakeAccessibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("FakeAccessibility")
            .field("active", &state.active)
            .field("published_count", &state.published)
            .finish_non_exhaustive()
    }
}

impl flui_semantics::platform::PlatformAccessibility for FakeAccessibility {
    fn publish(&self, _update: accesskit::TreeUpdate) {
        let mut state = self.state.lock();
        // Inactive means nothing is listening: the update is dropped rather
        // than counted, mirroring the real adapter's `update_if_active`.
        if state.active {
            state.published = state.published.saturating_add(1);
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

    /// No assistive technology asks a headless window for an action, so the
    /// listener is dropped here, outside any lock.
    fn set_action_listener(&self, listener: flui_semantics::platform::AccessibilityActionListener) {
        drop(listener);
    }

    /// The native headless host plays no screen reader; `flui-testing`'s does.
    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        flui_semantics::AssistiveFocusSupport::Unreported
    }
}

/// The recorder put nothing on its window and records no focus.
impl crate::host::WindowAccessibility for FakeAccessibility {
    fn detach(&self) {}

    fn focus_changed(&self, _: bool) {}
}

/// Each headless window gets a recorder of its own.
impl crate::host::Accessibility<()> for FakeAccessibility {
    fn for_window((): &()) -> std::sync::Arc<dyn crate::host::WindowAccessibility> {
        std::sync::Arc::new(Self::new())
    }
}
