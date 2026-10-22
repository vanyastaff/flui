//! The headless host: every position simulated in memory for tests.

use flui_platform::{InMemoryClipboard, haptics::FakeHaptics};

use crate::display::headless::{DisplaySpec, laid_out};
use crate::host::Host;
use crate::owner::headless::{HeadlessHost, HeadlessLoop};
use crate::platform::HostPlatform;
use crate::platforms::headless::{
    DragDouble, FakeAccessibility, FakeSystemChrome, HeadlessHostEvents, HeadlessInputDevices,
    HeadlessPreferences, MockWindow,
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
    type Displays = HeadlessLoop;
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

impl HeadlessHost for Headless {
    /// One display, 1920 by 1080 at scale 1.
    fn displays() -> flui_platform::DisplaySnapshot {
        laid_out(vec![DisplaySpec::default()])
    }
}

/// The headless platform: no windowing system, no graphics context; for unit
/// tests, CI without a display server and benchmarks without rendering.
pub type HeadlessPlatform = HostPlatform<Headless>;

impl HostPlatform<Headless> {
    /// A headless platform with one display and every window capability.
    #[must_use]
    pub fn new() -> Self {
        Self::from_loop(HeadlessLoop::new::<Headless>())
    }
}

impl Default for HostPlatform<Headless> {
    fn default() -> Self {
        Self::new()
    }
}
