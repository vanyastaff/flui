//! The headless host: every position simulated in memory for tests.

use std::sync::Arc;

use crate::host::MemoryClipboard;

use crate::accessibility::headless::FakeAccessibility;
use crate::display::headless::{DisplaySpec, laid_out};
use crate::drag::headless::{DragDouble, HeadlessDragSource};
use crate::drag::offers::OfferTable;
use crate::haptics::headless::HeadlessHaptics;
use crate::host::Host;
use crate::input::headless::{AttachedDevices, HeadlessInputDevices};
use crate::lifecycle::headless::HeadlessHostEvents;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::app_delivery::AppDelivery;
use crate::owner::headless::{
    HeadlessHost, HeadlessLoop, HeadlessState, SimulatedDevices, SimulatedOffers,
};
use crate::owner::notices::NoticeChannel;
use crate::platform::HostPlatform;
use crate::preferences::headless::HeadlessPreferences;
use crate::text::headless::FakeTextInput;
use crate::window::headless::{FakeSystemChrome, HeadlessWindows, MockWindow};

/// The headless host's table.
#[derive(Debug)]
pub struct Headless;

impl Host for Headless {
    const NAME: &'static str = "Headless";

    type Loop = HeadlessLoop;
    type Window = MockWindow;
    type Displays = HeadlessLoop;
    type Input = HeadlessInputDevices;
    type Text = FakeTextInput;
    type Accessibility = FakeAccessibility;
    type Clipboard = MemoryClipboard;
    type PrimarySelection = crate::host::Absent<crate::host::position::PrimarySelection>;
    type Drag = DragDouble;
    type Haptics = HeadlessHaptics;
    type Preferences = HeadlessPreferences;
    type Lifecycle = HeadlessHostEvents;
    type Chrome = FakeSystemChrome;
    type Storage = crate::storage::HeadlessData;
}

impl HeadlessHost for Headless {
    /// One display, 1920 by 1080 at scale 1.
    fn displays() -> flui_platform::DisplaySnapshot {
        laid_out(vec![DisplaySpec::default()])
    }

    fn notices(app: &Arc<AppDelivery>) -> Arc<dyn NoticeChannel> {
        Arc::new(SystemNotices::new(Arc::clone(app)))
    }

    fn devices() -> Box<dyn SimulatedDevices> {
        Box::new(AttachedDevices::default())
    }

    fn offers() -> Box<dyn SimulatedOffers> {
        Box::new(OfferTable::default())
    }

    fn drag_source(
        state: &HeadlessState,
    ) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        HeadlessDragSource::of(state)
    }
}

/// The headless platform: no windowing system, no graphics context; for unit
/// tests, CI without a display server and benchmarks without rendering.
pub type HeadlessPlatform = HostPlatform<Headless>;

impl HostPlatform<Headless> {
    /// A headless platform with one display and every window capability.
    #[must_use]
    pub fn new() -> Self {
        Self::from_parts(HeadlessWindows::new, |windows| {
            HeadlessLoop::new::<Headless>(Arc::clone(windows) as _)
        })
    }
}

impl Default for HostPlatform<Headless> {
    fn default() -> Self {
        Self::new()
    }
}
