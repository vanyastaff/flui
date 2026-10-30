//! The headless host's actuator, which every headless window hands out. It
//! is the host's receiver, not the scenario double: a test that asserts what
//! played takes `flui_platform::testing::FakeHaptics` (ADR-0243 D4).

use std::sync::Arc;

use flui_platform::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform::{DeviceId, HapticFeedback, PlatformHaptics};

use crate::host;

/// An actuator that plays both tiers exactly on its window and keeps
/// nothing: a headless window has no device of its own, so every request,
/// whatever its source, plays on the window's actuator.
#[derive(Debug, Default)]
pub struct HeadlessHaptics;

impl PlatformHaptics for HeadlessHaptics {
    fn perform(&self, _feedback: HapticFeedback) {}

    fn perform_with(&self, _feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        HapticOutcome::Submitted
    }

    fn play(
        &self,
        _pattern: &HapticPattern,
        _fallback: HapticFeedback,
        _request: &HapticRequest,
    ) -> HapticOutcome {
        HapticOutcome::Submitted
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        HapticCapabilities::FULL
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Each headless window gets an actuator of its own.
impl host::Haptics<()> for HeadlessHaptics {
    fn for_window((): &()) -> Arc<dyn PlatformHaptics> {
        Arc::new(Self)
    }
}
