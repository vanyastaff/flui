//! UIKit haptics: the feedback generators (ADR-0210 §7).
//!
//! From iOS 17.5 the generators bind to the window's view
//! (`feedbackGeneratorForView:`), so the feedback comes from the device the
//! view's scene is on; before it the view-less initializers play unaddressed
//! and the degradation table reports every outcome approximated. Core
//! Haptics, the second tier, needs `objc2-core-haptics`, which the workspace
//! does not carry yet: until it does, the engine is `Unsupported` and every
//! pattern plays its fallback.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};
use objc2_ui_kit::{
    UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
    UINotificationFeedbackType, UISelectionFeedbackGenerator, UIView,
};

use crate::shared::haptic_degradation::{
    CoreHapticsEngine, IosGenerator, IosImpact, IosNotification, ios_feedback, ios_pattern,
};
use flui_platform_api::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

/// The window's haptic actuator, bound to its view on the main thread.
pub(super) struct IOSHaptics {
    view: super::native_owner::NativeOwner<Retained<UIView>>,
}

impl std::fmt::Debug for IOSHaptics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IOSHaptics").finish_non_exhaustive()
    }
}

/// Whether the generators take a view (iOS 17.5+).
fn view_bound() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 17,
        minorVersion: 5,
        patchVersion: 0,
    })
}

const fn impact_style(impact: IosImpact) -> UIImpactFeedbackStyle {
    match impact {
        IosImpact::Light => UIImpactFeedbackStyle::Light,
        IosImpact::Medium => UIImpactFeedbackStyle::Medium,
        IosImpact::Rigid => UIImpactFeedbackStyle::Rigid,
        IosImpact::Soft => UIImpactFeedbackStyle::Soft,
        _ => UIImpactFeedbackStyle::Heavy,
    }
}

const fn notification_type(notification: IosNotification) -> UINotificationFeedbackType {
    match notification {
        IosNotification::Success => UINotificationFeedbackType::Success,
        IosNotification::Warning => UINotificationFeedbackType::Warning,
        _ => UINotificationFeedbackType::Error,
    }
}

impl IOSHaptics {
    pub(super) fn new(view: Retained<UIView>, mtm: MainThreadMarker) -> Self {
        Self {
            view: super::native_owner::NativeOwner::new(view, mtm),
        }
    }

    /// Play `generator`, through the view from iOS 17.5.
    #[expect(
        deprecated,
        reason = "the view-less initializers are the only ones before iOS 17.5"
    )]
    fn play_generator(&self, generator: IosGenerator, bound: bool, mtm: MainThreadMarker) {
        let view = self.view.get(mtm);
        match generator {
            IosGenerator::Selection => {
                let generator = if bound {
                    UISelectionFeedbackGenerator::feedbackGeneratorForView(view)
                } else {
                    UISelectionFeedbackGenerator::new(mtm)
                };
                generator.selectionChanged();
            }
            IosGenerator::Notification(notification) => {
                let generator = if bound {
                    UINotificationFeedbackGenerator::feedbackGeneratorForView(view)
                } else {
                    UINotificationFeedbackGenerator::new(mtm)
                };
                generator.notificationOccurred(notification_type(notification));
            }
            IosGenerator::Impact(impact) => {
                let style = impact_style(impact);
                let generator = if bound {
                    UIImpactFeedbackGenerator::feedbackGeneratorWithStyle_forView(style, view)
                } else {
                    UIImpactFeedbackGenerator::initWithStyle(mtm.alloc(), style)
                };
                generator.impactOccurred();
            }
        }
    }
}

impl PlatformHaptics for IOSHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        // Requests run on the owner thread, which UIKit makes the main thread.
        let Some(mtm) = MainThreadMarker::new() else {
            return HapticOutcome::Unavailable;
        };
        let bound = view_bound();
        let (generator, outcome) = ios_feedback(feedback, bound);
        self.play_generator(generator, bound, mtm);
        outcome
    }

    fn play(
        &self,
        _pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let outcome = ios_pattern(CoreHapticsEngine::Unsupported);
        match self.perform_with(fallback, request) {
            played if played.played() => outcome,
            other => other,
        }
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        HapticCapabilities::FEEDBACK_ONLY
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
