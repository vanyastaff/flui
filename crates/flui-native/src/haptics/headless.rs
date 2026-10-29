//! The headless host's actuator: the contract's recording double, which every
//! headless window hands out.

use std::sync::Arc;

use flui_platform::PlatformHaptics;
pub use flui_platform::haptics::FakeHaptics;

use crate::host;

/// Each headless window gets an actuator of its own, which plays every
/// request exactly and records it.
impl host::Haptics<()> for FakeHaptics {
    fn for_window((): &()) -> Arc<dyn PlatformHaptics> {
        Arc::new(Self::new())
    }
}
