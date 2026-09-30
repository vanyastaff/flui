//! Haptic feedback vocabulary.
//!
//! [`HapticFeedback`] names each kind of feedback a caller can request:
//! vibrate, light/medium/heavy impact, selection click, and the
//! success/warning/error notifications. Haptic vocabularies grow over time
//! (the notification kinds arrived after the original five on the
//! platforms that offer them), so [`HapticFeedback`] is `#[non_exhaustive]`:
//! a future platform-specific style is an additive variant, not a breaking
//! change.
//!
//! # Fire-and-forget, best-effort semantics
//!
//! Every variant is a **silent no-op** on a platform, OS version, or
//! device that has no corresponding haptic hardware or permission —
//! this is the degradation contract (a request is fire-and-forget and
//! never surfaces "unsupported" as an error). There is
//! deliberately no availability-discovery API: a caller cannot ask "can
//! this device vibrate?" before calling. See `PlatformHaptics` in
//! `flui-platform-api` for the capability trait a backend implements.
//!
//! # Why this type lives in the contract crate, not `flui-platform`
//!
//! Following the [`crate::ImeEvent`] precedent: the payload vocabulary a
//! platform capability carries is homed in `flui-platform-api` so crates below
//! `flui-platform` in the dependency graph (a future Material `InkWell`,
//! `Switch`, or other haptics-emitting widget in `flui-widgets`/
//! `flui-material`) can name the type without depending on the platform
//! layer itself. Only the trait that actually *performs* feedback
//! (`PlatformHaptics`) needs `flui-platform`.

/// A single haptic feedback request.
///
/// See the module docs for the fire-and-forget degradation contract every
/// variant shares.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HapticFeedback {
    /// A generic device vibration.
    Vibrate,
    /// A light tactile impact, e.g. a small/light UI element change.
    LightImpact,
    /// A medium tactile impact, e.g. a medium-weight UI element change.
    MediumImpact,
    /// A heavy tactile impact, e.g. a large/heavy UI element change.
    HeavyImpact,
    /// A selection change, e.g. scrolling through a picker.
    SelectionClick,
    /// A successful action/operation notification.
    SuccessNotification,
    /// A warning notification.
    WarningNotification,
    /// An error notification.
    ErrorNotification,
}
