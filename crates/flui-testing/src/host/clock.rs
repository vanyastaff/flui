//! The clock a headless host's test reads and advances.

use std::rc::Rc;

use std::time::Duration;

use flui_platform::capability::scripted::{Scenario, ScenarioDriver};

/// A headless host's clock as a test sees it: read it, and move it only with
/// [`advance`](Self::advance), which applies what the host's scenario has
/// due at the new instant. A clone is the same clock, so a widget's callback
/// may hold one; there is no way to move the time and skip the scenario.
#[derive(Clone)]
pub struct HostClock {
    pub(super) driver: Rc<ScenarioDriver>,
}

impl HostClock {
    /// The virtual time since the host started.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.driver.clock().elapsed()
    }

    /// Move the time forward by `dt` and apply every change of the host's
    /// scenario due at the new instant.
    ///
    /// # Panics
    ///
    /// With the first panic of a change it applied, once every due change has
    /// run.
    pub fn advance(&self, dt: Duration) {
        let _applied = self.driver.advance(dt);
    }

    pub(super) fn scenario(&self) -> &Scenario {
        self.driver.scenario()
    }
}

impl flui_foundation::MonotonicClock for HostClock {
    fn now(&self) -> flui_scheduler::Instant {
        flui_foundation::MonotonicClock::now(self.driver.clock())
    }
}

impl std::fmt::Debug for HostClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostClock")
            .field("elapsed", &self.elapsed())
            .finish()
    }
}
