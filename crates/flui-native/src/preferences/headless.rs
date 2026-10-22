//! The headless host's system preferences: the values a test sets, from the
//! start or from any thread while the platform runs.

use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::error::PlatformError;
use crate::host::{self, Host};
use crate::owner::headless::{HeadlessLoop, HeadlessState};
use crate::owner::signal::OwnerSignal;
use crate::platform::HostPlatform;

/// The headless platform's preference controls, taken before
/// [`Platform::run`](crate::Platform::run) consumes it.
impl<H: Host<Loop = HeadlessLoop>> HostPlatform<H> {
    /// What the system reports as its preferences from the start, before any
    /// window exists. The default reports every field unknown.
    #[must_use]
    pub fn with_preferences(self, values: flui_platform::SystemPreferences) -> Self {
        self.owner().with_state(|state| state.preferences = values);
        self
    }

    /// Obtain the preference source before `run` consumes this platform: the
    /// handle changes the system's preferences the way an operating system
    /// would, from any thread. See [`HeadlessPreferences`].
    #[must_use]
    pub fn preference_source(&self) -> HeadlessPreferences {
        HeadlessPreferences {
            signal: Arc::downgrade(&self.owner().signal),
            state: Arc::downgrade(&self.owner().state),
        }
    }
}

impl host::Preferences<HeadlessLoop> for HeadlessPreferences {
    fn sample(owner: &HeadlessLoop) -> Result<flui_platform::SystemPreferences, PlatformError> {
        owner.with_state(|state| match state.preference_failure.take() {
            Some(message) => Err(PlatformError::Preferences { message }),
            None => Ok(state.preferences.clone()),
        })
    }
}

/// The headless platform's system preferences after `run` has consumed it:
/// obtained from [`HeadlessPlatform::preference_source`](crate::HeadlessPlatform::preference_source) before `run`.
///
/// A change commits to the platform's preferences and then wakes the owner, as
/// a native source does when the OS reports a setting change, so it works from
/// any thread: the host reads `Platform::preferences` on its next turn and
/// delivers what it accepts to every runtime.
#[derive(Clone)]
pub struct HeadlessPreferences {
    signal: Weak<OwnerSignal>,
    state: Weak<Mutex<HeadlessState>>,
}

impl std::fmt::Debug for HeadlessPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessPreferences")
            .finish_non_exhaustive()
    }
}

impl HeadlessPreferences {
    /// Report `values` as the system's preferences and wake the owner. A
    /// platform that is gone applies nothing.
    pub fn set(&self, values: flui_platform::SystemPreferences) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        state.lock().preferences = values;
        self.wake();
    }

    /// Make the next read fail with `message`, as a native getter that cannot
    /// complete does, and wake the owner. The host keeps the values it accepted
    /// last; the read after that reports the platform's preferences again.
    pub fn fail_next_read(&self, message: impl Into<String>) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        state.lock().preference_failure = Some(message.into());
        self.wake();
    }

    fn wake(&self) {
        // A failed wake leaves the signal's pending debt for the next turn.
        if let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
    }
}
