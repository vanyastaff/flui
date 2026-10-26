//! A recording [`PlatformHaptics`] for tests and headless hosts.

use parking_lot::Mutex;

use super::{HapticOutcome, HapticPattern, HapticRequest, PatternPlayback};
use crate::{DeviceId, HapticFeedback, PlatformHaptics, Unsupported, UnsupportedReason};

use super::HapticCapabilities;

/// What a request asked a [`FakeHaptics`] to play.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum HapticPlay {
    /// A semantic kind.
    Feedback(HapticFeedback),
    /// A pattern, and the kind it falls back to.
    Pattern {
        /// The pattern.
        pattern: HapticPattern,
        /// Its tier-1 fallback.
        fallback: HapticFeedback,
    },
}

/// One request a [`FakeHaptics`] received and what it returned.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct HapticCall {
    /// What was asked.
    pub play: HapticPlay,
    /// The request as received.
    pub request: HapticRequest,
    /// The device whose actuator played it: the request's source when the
    /// fake knows that device, else `None`, the window's actuator.
    pub actuator: Option<DeviceId>,
    /// The outcome returned.
    pub outcome: HapticOutcome,
}

#[derive(Debug)]
struct State {
    calls: Vec<HapticCall>,
    devices: Vec<DeviceId>,
    capabilities: HapticCapabilities,
}

/// A recording fake for [`PlatformHaptics`] (ADR-0210): it records every
/// request and the outcome it returned, so a test asserts what reached the
/// platform and how.
///
/// By default the window's actuator plays everything exactly. A source device
/// counts only once [`with_device`](Self::with_device) names it; any other
/// source plays on the window's actuator, as a stale source does on a real
/// backend.
#[derive(Debug)]
pub struct FakeHaptics {
    state: Mutex<State>,
}

impl Default for FakeHaptics {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHaptics {
    /// A fake whose window actuator plays both tiers exactly.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capabilities(HapticCapabilities::FULL)
    }

    /// A fake whose actuators have `capabilities`: patterns play as
    /// [`HapticCapabilities::patterns`] says, and an actuator without
    /// [`HapticCapabilities::feedback`] is [`HapticOutcome::Unsupported`] (this
    /// device has none, [`UnsupportedReason::NotOnThisDevice`]).
    #[must_use]
    pub fn with_capabilities(capabilities: HapticCapabilities) -> Self {
        Self {
            state: Mutex::new(State {
                calls: Vec::new(),
                devices: Vec::new(),
                capabilities,
            }),
        }
    }

    /// This fake knowing `device` as an actuator of its own.
    #[must_use]
    pub fn with_device(self, device: DeviceId) -> Self {
        self.state.lock().devices.push(device);
        self
    }

    /// Every request, in arrival order.
    #[must_use]
    pub fn calls(&self) -> Vec<HapticCall> {
        self.state.lock().calls.clone()
    }

    /// The semantic kinds that played, in order: each tier-1 request, and the
    /// fallback of each pattern that fell back.
    #[must_use]
    pub fn feedback(&self) -> Vec<HapticFeedback> {
        self.state
            .lock()
            .calls
            .iter()
            .filter(|call| call.outcome.played())
            .filter_map(|call| match &call.play {
                HapticPlay::Feedback(kind) => Some(*kind),
                HapticPlay::Pattern { fallback, .. } => {
                    (call.outcome == HapticOutcome::FellBack).then_some(*fallback)
                }
            })
            .collect()
    }

    /// Forget every recorded request.
    pub fn clear(&self) {
        self.state.lock().calls.clear();
    }

    fn record(&self, play: HapticPlay, request: &HapticRequest) -> HapticOutcome {
        let mut state = self.state.lock();
        let actuator = request
            .source
            .filter(|source| state.devices.contains(source));
        let capabilities = state.capabilities;
        let outcome = match (&play, capabilities.feedback) {
            (_, false) => {
                HapticOutcome::Unsupported(Unsupported::new(UnsupportedReason::NotOnThisDevice))
            }
            (HapticPlay::Feedback(_), true) => HapticOutcome::Submitted,
            (HapticPlay::Pattern { .. }, true) => match capabilities.patterns {
                PatternPlayback::Exact => HapticOutcome::Submitted,
                PatternPlayback::Approximated => HapticOutcome::Approximated,
                PatternPlayback::FallsBack => HapticOutcome::FellBack,
            },
        };
        state.calls.push(HapticCall {
            play,
            request: *request,
            actuator,
            outcome,
        });
        outcome
    }
}

impl PlatformHaptics for FakeHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, request: &HapticRequest) -> HapticOutcome {
        self.record(HapticPlay::Feedback(feedback), request)
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        self.record(
            HapticPlay::Pattern {
                pattern: pattern.clone(),
                fallback,
            },
            request,
        )
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        self.state.lock().capabilities
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
