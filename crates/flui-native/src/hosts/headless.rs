//! The headless host: every position simulated in memory for tests.

use flui_platform::{InMemoryClipboard, haptics::FakeHaptics};

use crate::host::Host;
use crate::owner::headless::HeadlessLoop;
use crate::platform::HostPlatform;
use crate::platforms::headless::{
    DragDouble, FakeAccessibility, FakeSystemChrome, FakeTextInput, HeadlessDisplays,
    HeadlessHostEvents, HeadlessInputDevices, HeadlessPreferences, MockWindow,
};
use crate::storage::UserDataDirs;

/// The headless host's table.
#[derive(Debug)]
pub struct Headless;

impl Host for Headless {
    const NAME: &'static str = "Headless";

    type Loop = HeadlessLoop;
    type Window = MockWindow;
    type Displays = HeadlessDisplays;
    type Input = HeadlessInputDevices;
    type Text = FakeTextInput;
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
        Self::from_loop(HeadlessLoop::new())
    }
}

impl Default for HostPlatform<Headless> {
    fn default() -> Self {
        Self::new()
    }
}
