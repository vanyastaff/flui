//! Browser haptics: the Vibration API (ADR-0210 §7).
//!
//! `navigator.vibrate` plays on/off pulses, in Chromium only, and only once
//! the page has had user activation; Safari and Firefox have no vibration.
//! The degradation table turns each kind and pattern into pulses; a page
//! that cannot vibrate reports [`HapticOutcome::Unavailable`].

use js_sys::{Array, Reflect};
use wasm_bindgen::JsValue;

use crate::shared::haptic_degradation::{WebVibration, web_feedback, web_pattern};
use flui_platform_api::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, PatternPlayback,
};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

/// The page's haptic actuator.
#[derive(Debug, Default)]
pub(super) struct WebHaptics;

/// Whether the page can vibrate now: `navigator.vibrate` exists and the page
/// has had user activation (`navigator.userActivation.hasBeenActive`, the
/// sticky activation Chromium requires). Read through `Reflect`, since a
/// browser without either property must not throw.
fn vibration(navigator: &JsValue) -> WebVibration {
    let has = |target: &JsValue, name: &str| {
        Reflect::get(target, &JsValue::from_str(name))
            .ok()
            .filter(|value| !value.is_undefined() && !value.is_null())
    };
    let can_vibrate = has(navigator, "vibrate").is_some();
    let activated = has(navigator, "userActivation")
        .and_then(|activation| has(&activation, "hasBeenActive"))
        .and_then(|active| active.as_bool())
        .unwrap_or(false);
    if can_vibrate && activated {
        WebVibration::Available
    } else {
        WebVibration::Unavailable
    }
}

/// Play `pulses` (alternating on and off milliseconds) and report `outcome`,
/// or [`HapticOutcome::Unavailable`] when the browser refuses them.
fn vibrate(pulses: &[u32], outcome: HapticOutcome) -> HapticOutcome {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return HapticOutcome::Unavailable;
    };
    let pattern: Array = pulses.iter().map(|&millis| JsValue::from(millis)).collect();
    if navigator.vibrate_with_pattern(&pattern) {
        outcome
    } else {
        HapticOutcome::Unavailable
    }
}

fn page_vibration() -> WebVibration {
    web_sys::window().map_or(WebVibration::Unavailable, |window| {
        vibration(&JsValue::from(window.navigator()))
    })
}

impl PlatformHaptics for WebHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        match web_feedback(feedback, page_vibration()) {
            Some((pulses, outcome)) => vibrate(&pulses, outcome),
            None => HapticOutcome::Unavailable,
        }
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        match web_pattern(pattern, page_vibration()) {
            None => HapticOutcome::Unavailable,
            Some((_, HapticOutcome::FellBack)) => match self.perform_with(fallback, request) {
                outcome if outcome.played() => HapticOutcome::FellBack,
                outcome => outcome,
            },
            Some((pulses, outcome)) => vibrate(&pulses, outcome),
        }
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        match page_vibration() {
            WebVibration::Available => {
                HapticCapabilities::new(true, PatternPlayback::Approximated, false, false)
            }
            _ => HapticCapabilities::NONE,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
