//! The haptics rows (ADR-0210): what a backend's actuator may report.

use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use flui_platform_api::haptics::{
    HapticOutcome, HapticPattern, HapticRequest, HapticTiming, PatternPlayback,
};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

use super::rows::Outcome;
use super::{BackendUnderTest, RowOutcome};

const NO_ACTUATOR: RowOutcome = RowOutcome::NotApplicable("the backend has no haptic actuator");

const TIMINGS: [HapticTiming; 2] = [HapticTiming::Immediate, HapticTiming::FramePresented];

/// Two transients, which every backend that plays patterns can sequence.
fn taps() -> HapticPattern {
    HapticPattern::builder()
        .transient(Duration::ZERO, 1.0, 0.8)
        .transient(Duration::from_millis(60), 0.4, 0.2)
        .build()
        .expect("BUG: the conformance pattern is valid")
}

/// A device no backend has seen: a source put away or unplugged.
fn stale_device() -> DeviceId {
    DeviceId::new(NonZeroU64::new(0x00f1_0e57).expect("BUG: literal is nonzero"))
}

fn actuator<B: BackendUnderTest>(backend: &mut B) -> Result<Arc<dyn PlatformHaptics>, Outcome> {
    backend.haptics().ok_or(Ok(NO_ACTUATOR))
}

/// Every request of both tiers at both timings returns a final outcome:
/// only the frame queue above a backend queues.
pub(super) fn a_backend_never_queues<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let haptics = match actuator(backend) {
        Ok(haptics) => haptics,
        Err(outcome) => return outcome,
    };
    for timing in TIMINGS {
        let request = HapticRequest::new(timing);
        for kind in HapticFeedback::ALL {
            if let HapticOutcome::Queued(_) = haptics.perform_with(kind, &request) {
                return Err(format!("{kind:?} at {timing:?} was queued by the backend"));
            }
        }
        if let HapticOutcome::Queued(_) =
            haptics.play(&taps(), HapticFeedback::LightImpact, &request)
        {
            return Err(format!("a pattern at {timing:?} was queued by the backend"));
        }
    }
    Ok(RowOutcome::Held)
}

/// Tier 1 plays, plays approximated, or has no actuator; it never reports a
/// fallback, and suppression is the runtime's decision, not the backend's.
pub(super) fn semantic_feedback_reports_a_tier_one_outcome<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let haptics = match actuator(backend) {
        Ok(haptics) => haptics,
        Err(outcome) => return outcome,
    };
    for kind in HapticFeedback::ALL {
        let outcome = haptics.perform_with(kind, &HapticRequest::default());
        if !matches!(
            outcome,
            HapticOutcome::Submitted | HapticOutcome::Approximated | HapticOutcome::Unavailable
        ) {
            return Err(format!("{kind:?} reported {outcome:?}"));
        }
    }
    let outcome = haptics.play(
        &taps(),
        HapticFeedback::LightImpact,
        &HapticRequest::default(),
    );
    if outcome == HapticOutcome::Suppressed {
        return Err("a pattern was suppressed by the backend".to_owned());
    }
    Ok(RowOutcome::Held)
}

/// A source the backend does not know plays on the window's actuator: every
/// request reports what the same request without a source reports.
pub(super) fn a_stale_source_plays_on_the_window_actuator<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let haptics = match actuator(backend) {
        Ok(haptics) => haptics,
        Err(outcome) => return outcome,
    };
    let window = HapticRequest::default();
    let stale = window.with_source(stale_device());
    for kind in HapticFeedback::ALL {
        let expected = haptics.perform_with(kind, &window);
        let actual = haptics.perform_with(kind, &stale);
        if actual != expected {
            return Err(format!(
                "{kind:?} from a stale source reported {actual:?}, the window's actuator {expected:?}"
            ));
        }
    }
    let expected = haptics.play(&taps(), HapticFeedback::Drop, &window);
    let actual = haptics.play(&taps(), HapticFeedback::Drop, &stale);
    if actual != expected {
        return Err(format!(
            "a pattern from a stale source reported {actual:?}, the window's actuator {expected:?}"
        ));
    }
    Ok(RowOutcome::Held)
}

/// What the actuator's capabilities claim is what it plays: semantic
/// feedback exactly when it claims it, and patterns as it claims.
pub(super) fn capabilities_match_what_plays<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let haptics = match actuator(backend) {
        Ok(haptics) => haptics,
        Err(outcome) => return outcome,
    };
    let capabilities = haptics.capabilities(None);
    let request = HapticRequest::default();
    let feedback = haptics.perform_with(HapticFeedback::SelectionClick, &request);
    if capabilities.feedback == (feedback == HapticOutcome::Unavailable) {
        return Err(format!(
            "claims feedback {} but a selection click reported {feedback:?}",
            capabilities.feedback
        ));
    }
    let pattern = haptics.play(&taps(), HapticFeedback::SelectionClick, &request);
    let expected = match (capabilities.feedback, capabilities.patterns) {
        (false, _) => HapticOutcome::Unavailable,
        (true, PatternPlayback::Exact) => HapticOutcome::Submitted,
        (true, PatternPlayback::Approximated) => HapticOutcome::Approximated,
        (true, _) => HapticOutcome::FellBack,
    };
    if pattern != expected {
        return Err(format!(
            "claims {:?} patterns but two taps reported {pattern:?}",
            capabilities.patterns
        ));
    }
    Ok(RowOutcome::Held)
}
