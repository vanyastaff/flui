//! The headless host: every position simulated in memory for tests.

use std::sync::Arc;

use flui_platform::{InMemoryClipboard, haptics::FakeHaptics};

use crate::host::Host;
use crate::owner::headless::HeadlessLoop;
use crate::owner::positions::PositionObjects;
use crate::platform::HostPlatform;
use crate::platforms::headless::{
    DragDouble, FakeAccessibility, FakeSystemChrome, HeadlessDisplays, HeadlessHostEvents,
    HeadlessInputDevices, HeadlessPreferences, MockWindow,
};
use crate::storage::UserDataDirs;
use crate::text::headless::FakeTextInput;

/// The headless host's table.
#[derive(Debug)]
pub struct Headless;

impl Host for Headless {
    const NAME: &'static str = "Headless";

    type Loop = HeadlessLoop;
    type Window = MockWindow;
    type Displays = HeadlessDisplays;
    type Input = HeadlessInputDevices;
    type Text = std::sync::Arc<FakeTextInput>;
    type Accessibility = FakeAccessibility;
    type Clipboard = InMemoryClipboard;
    type Drag = DragDouble;
    type Haptics = FakeHaptics;
    type Preferences = HeadlessPreferences;
    type Lifecycle = HeadlessHostEvents;
    type Chrome = FakeSystemChrome;
    type Storage = UserDataDirs;
}

/// The headless platform: no windowing system, no graphics context; for unit
/// tests, CI without a display server and benchmarks without rendering.
pub type HeadlessPlatform = HostPlatform<Headless>;

impl HostPlatform<Headless> {
    /// A headless platform with one display and every window capability.
    #[must_use]
    pub fn new() -> Self {
        let drag = Arc::new(DragDouble::new());
        let owner = HeadlessLoop::new(PositionObjects::default().with(Arc::clone(&drag)));
        // The double delivers a session's events to the loop's windows.
        drag.attach(Arc::downgrade(&owner.state));
        Self::from_loop(owner)
    }
}

impl Default for HostPlatform<Headless> {
    fn default() -> Self {
        Self::new()
    }
}
