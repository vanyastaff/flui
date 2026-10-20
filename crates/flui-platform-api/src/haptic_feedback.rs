//! Haptic feedback vocabulary, the semantic tier (ADR-0210 §1).
//!
//! [`HapticFeedback`] names each kind of feedback a caller can request by
//! meaning: an impact, a selection change, a notification, an alignment snap,
//! a level change, a lift, a drop, a confirmation or a rejection. Haptic
//! vocabularies grow (AppKit, Android's `HapticFeedbackConstants` and SwiftUI's
//! `SensoryFeedback` each name kinds the others lack), so the enum is
//! `#[non_exhaustive]`: a new kind is an additive variant.
//!
//! A kind the device has no equivalent for plays as the nearest kind the
//! backend has, and the request reports
//! [`HapticOutcome::Approximated`](crate::haptics::HapticOutcome::Approximated).
//! Pattern playback, the second tier, lives in [`haptics`](crate::haptics).
//!
//! # Why this type lives in the contract crate, not `flui-platform`
//!
//! Following the [`crate::ImeEvent`] precedent: the payload vocabulary a
//! platform capability carries is homed in `flui-platform-api` so crates below
//! `flui-platform` in the dependency graph (a haptics-emitting widget in
//! `flui-widgets` or `flui-material`) can name the type without depending on
//! the platform layer itself.

/// One semantic haptic feedback kind.
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
    /// Something snapped into alignment with a guide (AppKit's `alignment`).
    Alignment,
    /// A value crossed a discrete level, such as a pressure stage or a
    /// stepped slider (AppKit's `levelChange`).
    LevelChange,
    /// A dragged item lifted off its place (Android's `DRAG_START`).
    DragLift,
    /// A dragged item was dropped onto a target.
    Drop,
    /// An action was accepted (Android's `CONFIRM`).
    Confirm,
    /// An action was refused (Android's `REJECT`).
    Reject,
}

impl HapticFeedback {
    /// Every kind, in declaration order: for tables that map each one.
    pub const ALL: [Self; 14] = [
        Self::Vibrate,
        Self::LightImpact,
        Self::MediumImpact,
        Self::HeavyImpact,
        Self::SelectionClick,
        Self::SuccessNotification,
        Self::WarningNotification,
        Self::ErrorNotification,
        Self::Alignment,
        Self::LevelChange,
        Self::DragLift,
        Self::Drop,
        Self::Confirm,
        Self::Reject,
    ];
}
