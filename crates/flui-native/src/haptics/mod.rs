//! Haptics: the feedback a window plays on the device's actuator (ADR-0210,
//! ADR-0151 §10).
//!
//! The model is [`degradation`], every backend's degradation table as plain
//! functions: what plays, approximated or falls back, for each feedback kind
//! and pattern, checked on every host. Beside it, one file per platform holds
//! the actuator a window hands out, the type the haptics position of its
//! table names:
//!
//! - `windows`: the pen in range of the window;
//! - `apple`: `NSHapticFeedbackManager` on macOS, the feedback generators and
//!   Core Haptics on iOS;
//! - `android`: the view's haptic feedback and the vibrator;
//! - `web`: `navigator.vibrate`;
//! - `headless`: the contract's recording double.
//!
//! Linux and winit have no actuator to drive: their tables name
//! `Absent<Haptics>`.

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
pub mod degradation;
pub(crate) mod headless;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
