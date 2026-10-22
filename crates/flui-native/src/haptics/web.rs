//! Browser haptics: the Vibration API (ADR-0210 §7).
//!
//! `navigator.vibrate` plays on/off pulses, in Chromium only, and only once
//! the page has had user activation; Safari and Firefox have no vibration.
//! The degradation table turns each kind and pattern into pulses; a page
//! that cannot vibrate reports [`HapticOutcome::Unsupported`]: `NotOnThisDevice`
//! for a browser without vibration, `Unavailable` before user activation.

use js_sys::{Array, Reflect};
use wasm_bindgen::JsValue;

use crate::haptics::degradation::{WebVibration, web_feedback, web_pattern};
use flui_platform::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, PatternPlayback,
};
use flui_platform::{DeviceId, HapticFeedback, PlatformHaptics, Unsupported, UnsupportedReason};

/// The page's haptic actuator.
#[derive(Debug, Default)]
pub struct WebHaptics;

/// Whether the page can vibrate now, or why not: its browser has no
/// `navigator.vibrate` (Safari, Firefox), or the page has had no user
/// activation yet (`navigator.userActivation.hasBeenActive`, the sticky
/// activation Chromium requires), which a later request may have. Read
/// through `Reflect`, since a browser without either property must not throw.
fn vibration(navigator: &JsValue) -> Result<(), UnsupportedReason> {
    let has = |target: &JsValue, name: &str| {
        Reflect::get(target, &JsValue::from_str(name))
            .ok()
            .filter(|value| !value.is_undefined() && !value.is_null())
    };
    if has(navigator, "vibrate").is_none() {
        return Err(UnsupportedReason::NotOnThisDevice);
    }
    let activated = has(navigator, "userActivation")
        .and_then(|activation| has(&activation, "hasBeenActive"))
        .and_then(|active| active.as_bool())
        .unwrap_or(false);
    if activated {
        Ok(())
    } else {
        Err(UnsupportedReason::Unavailable)
    }
}

/// Play `pulses` (alternating on and off milliseconds) and report `outcome`,
/// or [`HapticOutcome::Unsupported`] when the browser refuses them.
fn vibrate(pulses: &[u32], outcome: HapticOutcome) -> HapticOutcome {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return Unsupported::new(UnsupportedReason::NoWindow).into();
    };
    let pattern: Array = pulses.iter().map(|&millis| JsValue::from(millis)).collect();
    if navigator.vibrate_with_pattern(&pattern) {
        outcome
    } else {
        Unsupported::new(UnsupportedReason::Unavailable).into()
    }
}

/// Whether the page can vibrate now, or why not: with no window there is no
/// page.
fn page_vibration() -> Result<(), UnsupportedReason> {
    web_sys::window().map_or(Err(UnsupportedReason::NoWindow), |window| {
        vibration(&JsValue::from(window.navigator()))
    })
}

/// The page's vibration as the degradation table takes it, and what a request
/// reports when nothing can play.
fn page() -> (WebVibration, HapticOutcome) {
    match page_vibration() {
        Ok(()) => (
            WebVibration::Available,
            Unsupported::new(UnsupportedReason::Unavailable).into(),
        ),
        Err(reason) => (WebVibration::Unavailable, Unsupported::new(reason).into()),
    }
}

impl PlatformHaptics for WebHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, _request: &HapticRequest) -> HapticOutcome {
        let (vibration, silent) = page();
        match web_feedback(feedback, vibration) {
            Some((pulses, outcome)) => vibrate(&pulses, outcome),
            None => silent,
        }
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let (vibration, silent) = page();
        match web_pattern(pattern, vibration) {
            None => silent,
            Some((_, HapticOutcome::FellBack)) => match self.perform_with(fallback, request) {
                outcome if outcome.played() => HapticOutcome::FellBack,
                outcome => outcome,
            },
            Some((pulses, outcome)) => vibrate(&pulses, outcome),
        }
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        match page_vibration() {
            Ok(()) => HapticCapabilities::new(true, PatternPlayback::Approximated, false, false),
            Err(_) => HapticCapabilities::NONE,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
