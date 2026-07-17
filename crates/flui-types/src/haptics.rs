//! Haptic feedback vocabulary.
//!
//! [`HapticFeedback`] mirrors Flutter's `HapticFeedback` static methods
//! 1:1 (`packages/flutter/lib/src/services/haptic_feedback.dart` @ 3.44.0):
//! `vibrate()`, `lightImpact()`, `mediumImpact()`, `heavyImpact()`,
//! `selectionClick()`, and `successNotification()`/`warningNotification()`/
//! `errorNotification()` — three more plain statics, added later, each
//! sending its own `'HapticFeedbackType.*Notification'` platform-channel
//! payload string (there is no `notification(type)` method upstream to
//! group them under) — Flutter's own vocabulary grew once already after
//! the original five landed. [`HapticFeedback`] is `#[non_exhaustive]` for
//! the same reason: a future upstream or platform-specific style is an
//! additive variant here, not a breaking change.
//!
//! # Fire-and-forget, best-effort semantics
//!
//! Every variant is a **silent no-op** on a platform, OS version, or
//! device that has no corresponding haptic hardware or permission —
//! this is Flutter's own degradation contract (`HapticFeedback`'s
//! platform channel calls are fire-and-forget; the Dart API returns
//! `Future<void>` and never surfaces "unsupported" as an error). There is
//! deliberately no availability-discovery API upstream, and none is added
//! here: a caller cannot ask "can this device vibrate?" before calling,
//! matching Flutter's `HapticFeedback` exactly. See `PlatformHaptics` in
//! `flui-platform` for the capability trait that performs feedback (not
//! linked here — `flui-types` has no dependency on `flui-platform`).
//!
//! # Why this type lives in `flui-types`, not `flui-platform`
//!
//! Following the [`crate::ImeEvent`] precedent: the payload vocabulary a
//! platform capability carries is homed in `flui-types` so crates below
//! `flui-platform` in the dependency graph (a future Material `InkWell`,
//! `Switch`, or other haptics-emitting widget in `flui-widgets`/
//! `flui-material`) can name the type without depending on the platform
//! layer itself. Only the trait that actually *performs* feedback
//! (`PlatformHaptics`) needs `flui-platform`.

/// A single haptic feedback request, matching Flutter's `HapticFeedback`
/// static method vocabulary.
///
/// See the module docs for the fire-and-forget degradation contract every
/// variant shares.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HapticFeedback {
    /// A generic device vibration. Mirrors `HapticFeedback.vibrate()`.
    Vibrate,
    /// A light tactile impact, e.g. a small/light UI element change.
    /// Mirrors `HapticFeedback.lightImpact()`.
    LightImpact,
    /// A medium tactile impact, e.g. a medium-weight UI element change.
    /// Mirrors `HapticFeedback.mediumImpact()`.
    MediumImpact,
    /// A heavy tactile impact, e.g. a large/heavy UI element change.
    /// Mirrors `HapticFeedback.heavyImpact()`.
    HeavyImpact,
    /// A selection change, e.g. scrolling through a picker.
    /// Mirrors `HapticFeedback.selectionClick()`.
    SelectionClick,
    /// A successful action/operation notification.
    /// Mirrors `HapticFeedback.successNotification()`.
    SuccessNotification,
    /// A warning notification.
    /// Mirrors `HapticFeedback.warningNotification()`.
    WarningNotification,
    /// An error notification.
    /// Mirrors `HapticFeedback.errorNotification()`.
    ErrorNotification,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_copy_eq_and_hashable() {
        // Smoke test for the derive set: `Copy` (no explicit `.clone()`
        // needed to reuse a value), `PartialEq`/`Eq` (so a recording fake
        // can assert delivery order by direct comparison), and `Hash` (so
        // a future dedup/counter map over feedback kinds is possible).
        let a = HapticFeedback::SelectionClick;
        let b = a; // Copy, not a move
        assert_eq!(a, b);

        let mut seen = std::collections::HashSet::new();
        seen.insert(HapticFeedback::Vibrate);
        seen.insert(HapticFeedback::Vibrate);
        assert_eq!(seen.len(), 1, "Hash + Eq must agree on equal variants");
    }

    #[test]
    fn distinct_variants_are_not_equal() {
        assert_ne!(HapticFeedback::LightImpact, HapticFeedback::MediumImpact);
        assert_ne!(HapticFeedback::HeavyImpact, HapticFeedback::Vibrate);
        assert_ne!(
            HapticFeedback::SuccessNotification,
            HapticFeedback::WarningNotification
        );
        assert_ne!(
            HapticFeedback::WarningNotification,
            HapticFeedback::ErrorNotification
        );
    }
}
