//! The build pipeline behind `flui build` and `flui run`: cross-platform
//! builds for Android, Web (WASM), iOS and desktop, plus platform scaffolding.
//! Formerly the `flui-build` crate; it has no consumer but this binary, so it
//! lives here as a module.
//!
//! Each platform is one builder type (`AndroidBuilder`, `IosBuilder`,
//! `WebBuilder`, `DesktopBuilder`) with the same three entry points:
//! `validate_environment`, `build_rust`, `build_platform`. The set is closed
//! and `commands/build.rs` dispatches on the target with a `match`, so there
//! is no trait behind them.

/// Android platform build support
pub(crate) mod android;
/// APK packaging with the SDK build-tools (no Gradle).
mod android_package;
/// Type-state builder for `BuilderContext`
pub(crate) mod context_builder;
/// Desktop platform build support (Windows, macOS, Linux)
pub(crate) mod desktop;
/// Custom error types for build operations
pub(crate) mod error;
/// iOS platform build support
pub(crate) mod ios;
mod ios_package;
/// Platform abstractions and core types
pub(crate) mod platform;
/// Platform scaffolding for new projects
pub(crate) mod scaffold;
/// Utility functions and helpers
pub(crate) mod util;

#[cfg(test)]
mod tests;
/// Web/WASM platform build support
pub(crate) mod web;

pub(crate) use android::AndroidBuilder;
pub(crate) use context_builder::BuilderContextBuilder;
pub(crate) use desktop::DesktopBuilder;
pub(crate) use ios::IosBuilder;
pub(crate) use platform::*;
pub(crate) use web::WebBuilder;
