//! Haptics: the platform capability and its two tiers (ADR-0210).
//!
//! FLUI has no standalone `services` layer (`docs/FOUNDATIONS.md`); haptics
//! is a capability trait, [`PlatformHaptics`], reached through
//! [`PlatformWindow::haptics`](crate::PlatformWindow::haptics), a fallible
//! accessor returning `Option<Arc<dyn _>>`. A backend with no haptic hardware
//! (desktop winit, Linux) returns `None` instead of every window inheriting a
//! method it cannot honor.
//!
//! # Two tiers, one request, one outcome
//!
//! Tier 1 is semantic: a [`HapticFeedback`] kind. Tier 2 is a
//! [`HapticPattern`] of transient and continuous events with normalized
//! [`Intensity`] and [`Sharpness`], the model Core Haptics and Android's
//! envelopes share. Both take a [`HapticRequest`] naming the actuator, the
//! timing and the input sequence, and both return a [`HapticOutcome`] a test
//! can assert. A pattern always names the tier-1 kind it falls back to.
//!
//! This module is always built, so Cargo's feature unification cannot change
//! it; the user-facing gate is the additive `unstable-input` feature on the
//! `flui-sdk` and facade re-exports (ADR-0210 §9). [`PlatformHaptics`] and
//! [`HapticFeedback`] are Stable and live at the crate root.
//!
//! # One method per tier, not one per kind
//!
//! Every [`HapticFeedback`] variant is the same operation with a different
//! kind, so the tier is one method taking the enum: a new kind is an additive
//! variant, not a breaking change to the trait. The methods the second tier
//! added have default bodies, so a new method never breaks an implementor.
//!
//! # Per-window, not device-global
//!
//! Android's `View.performHapticFeedback` is a method on a view, AppKit's
//! performer and UIKit's view-bound generators address a window, and Windows
//! routes haptics per thread to the window that received the input, so the
//! capability is reached from a window. A backend with one device-global
//! engine returns the same `Arc` from every window.

mod fake;
mod pattern;
mod request;

pub use fake::{FakeHaptics, HapticCall, HapticPlay};
pub use pattern::{
    ControlPoint, HapticEvent, HapticPattern, HapticPatternBuilder, HapticPatternError, Intensity,
    ParameterCurve, Sharpness, UnitRangeError,
};
pub use request::{
    HapticCapabilities, HapticOutcome, HapticRequest, HapticTicket, HapticTiming, PatternPlayback,
};

use crate::{DeviceId, HapticFeedback};

/// Platform capability for haptic feedback on one window.
///
/// A backend plays a request when it is called: it never returns
/// [`HapticOutcome::Queued`], which only the frame queue above it returns, and
/// it treats [`HapticTiming::FramePresented`] as now, since the queue calls it
/// when that frame is presented. A source device it does not know plays on the
/// window's actuator (ADR-0210 §5). It never stretches a duration, raises an
/// intensity, or plays for a hidden window.
pub trait PlatformHaptics: Send + Sync {
    /// Perform `feedback` on this window, best-effort and unaddressed.
    ///
    /// The Stable tier-1 entry point; [`perform_with`](Self::perform_with)
    /// adds the request and reports the outcome.
    fn perform(&self, feedback: HapticFeedback);

    /// Perform `feedback` for `request` and report what became of it.
    ///
    /// The default performs it unaddressed and reports
    /// [`HapticOutcome::Submitted`].
    fn perform_with(&self, feedback: HapticFeedback, request: &HapticRequest) -> HapticOutcome {
        let _ = request;
        self.perform(feedback);
        HapticOutcome::Submitted
    }

    /// Play `pattern` for `request`, or `fallback` where the pattern cannot
    /// play, and report what became of it.
    ///
    /// The default plays `fallback` and reports [`HapticOutcome::FellBack`],
    /// or the fallback's own outcome when it did not play either.
    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let _ = pattern;
        match self.perform_with(fallback, request) {
            outcome if outcome.played() => HapticOutcome::FellBack,
            outcome => outcome,
        }
    }

    /// What the actuator of `source` (the window's for `None` or an unknown
    /// device) can play. Read when the device set changes, never per request.
    ///
    /// The default is [`HapticCapabilities::FEEDBACK_ONLY`].
    fn capabilities(&self, source: Option<DeviceId>) -> HapticCapabilities {
        let _ = source;
        HapticCapabilities::FEEDBACK_ONLY
    }

    /// Downcast support for tests that need to reach a concrete recording
    /// fake (such as [`FakeHaptics`]) behind the trait object
    /// `PlatformWindow::haptics` returns.
    fn as_any(&self) -> &dyn std::any::Any;
}
