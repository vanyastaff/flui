//! Platform haptic feedback capability
//!
//! Flutter's `services` package is deliberately dissolved in FLUI
//! (`docs/FOUNDATIONS.md`); its haptics residue becomes a capability trait
//! here, following the identical template `PlatformTextInput`'s module doc
//! promised: [`PlatformHaptics`] is reached through
//! [`PlatformWindow::haptics`](super::window::PlatformWindow::haptics) — a
//! fallible accessor returning `Option<Arc<dyn _>>`, not a method bolted
//! directly onto `PlatformWindow` with a panicking/no-op default. A backend
//! with no haptic hardware (desktop winit; a minimal future embedder)
//! returns `None` from the accessor instead of every `PlatformWindow`
//! implementor inheriting a `perform` method it cannot honor.
//!
//! # One `perform(enum)` method, not eight discrete methods
//!
//! [`PlatformTextInput`](super::text_input::PlatformTextInput) exposes one
//! method per composition control (`set_ime_allowed`,
//! `set_ime_cursor_area`) because those controls are semantically distinct
//! operations with different argument shapes. Haptics is different: every
//! [`flui_types::HapticFeedback`] variant is the *same* operation ("perform
//! this feedback kind") with no argument beyond which kind. Eight discrete
//! `fn vibrate(&self)`, `fn light_impact(&self)`, ... methods would make
//! adding a ninth kind (Flutter's own vocabulary already grew once, see
//! [`flui_types::HapticFeedback`]'s module doc) a breaking change to this
//! trait. A single `perform(&self, HapticFeedback)` makes the same addition
//! a non-breaking enum variant instead — deliberately diverging from
//! `PlatformTextInput`'s discrete-method shape because the two capabilities
//! have different growth profiles, not by oversight.
//!
//! # Per-window, not device-global
//!
//! `PlatformHaptics` is reached from [`PlatformWindow`](super::window::PlatformWindow),
//! not from [`Platform`](super::platform::Platform) as a device-global
//! capability, for three reasons:
//!
//! 1. **Template consistency.** `PlatformTextInput`'s own module doc commits
//!    `PlatformSystemChrome`/`PlatformHaptics` to "the same template" —
//!    a fallible per-window accessor, matching `PlatformWindow::display`.
//! 2. **The richest target is per-`View`.** Android's haptic feedback API
//!    (`android.view.View.performHapticFeedback`) is a method on `View`, not
//!    a device-global service — of FLUI's eventual backend targets, Android
//!    is the one with the most granular haptics contract, and per-window is
//!    the FLUI shape closest to that reality.
//! 3. **It is the only scope `flui-app` can reach today.** `AppBinding`
//!    retains `active_window` as its one live platform handle —
//!    `Platform`/`Box<dyn Platform>` is consumed by `run()` and not kept
//!    around — so a device-global accessor on `Platform` would be unreachable
//!    from the one production bridge this capability needs
//!    (`AppBinding::perform_haptic_feedback`).
//!
//! Desktop backends with a single device-global haptics engine (if one ever
//! exists) can trivially satisfy this by returning the same `Arc` from every
//! window's accessor — per-window is not per-window-*state*, just
//! per-window-*reachability*.

use flui_types::HapticFeedback;

/// Platform capability for performing haptic feedback on one window.
///
/// See the module docs for why this is a single `perform(HapticFeedback)`
/// method (not eight discrete methods) reached per-window (not
/// device-global).
///
/// # Fire-and-forget, best-effort
///
/// `perform` has no return value and cannot fail from a caller's
/// perspective: a device/OS/permission combination that cannot honor the
/// request performs no feedback and returns nothing to indicate that,
/// mirroring Flutter's own `HapticFeedback` degradation contract (see
/// [`flui_types::HapticFeedback`]'s module doc).
pub trait PlatformHaptics: Send + Sync {
    /// Perform the given haptic feedback on this window, best-effort.
    fn perform(&self, feedback: HapticFeedback);

    /// Downcast support for tests that need to reach a concrete recording
    /// fake (e.g. the headless backend's `FakeHaptics`) behind the trait
    /// object `PlatformWindow::haptics` returns.
    fn as_any(&self) -> &dyn std::any::Any;
}
