//! AppKit haptics: `NSHapticFeedbackManager`'s default performer (ADR-0210 §7).
//!
//! The performer plays one of three patterns on a Force Touch trackpad and
//! no patterns of FLUI's second tier, so the degradation table maps every
//! kind and falls every pattern back. The frame queue already aligns a
//! request with its presented frame, so the performer is asked to play now.
//! A Mac without a Force Touch trackpad ignores the request; AppKit offers
//! no query to tell, so the outcome is the table's.

use objc2_app_kit::{
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer,
};

use crate::shared::haptic_degradation::{MacFeedback, macos_feedback, macos_pattern};
use flui_platform::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform::{DeviceId, HapticFeedback, PlatformHaptics};

/// The window's haptic actuator. Requests reach it on the owner thread, which
/// on AppKit is the main thread.
#[derive(Debug, Default)]
pub struct MacHaptics;

impl PlatformHaptics for MacHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        let (pattern, outcome) = macos_feedback(feedback);
        let pattern = match pattern {
            MacFeedback::Alignment => NSHapticFeedbackPattern::Alignment,
            MacFeedback::LevelChange => NSHapticFeedbackPattern::LevelChange,
            _ => NSHapticFeedbackPattern::Generic,
        };
        NSHapticFeedbackManager::defaultPerformer()
            .performFeedbackPattern_performanceTime(pattern, NSHapticFeedbackPerformanceTime::Now);
        outcome
    }

    fn play(
        &self,
        _pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let _ = self.perform_with(fallback, request);
        macos_pattern()
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        HapticCapabilities::FEEDBACK_ONLY
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
