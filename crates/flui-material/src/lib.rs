//! # `flui_material`
//!
//! Material Design theming foundation for FLUI. This first slice ships
//! [`ColorScheme`] — the full Material 3 color-role palette. `Typography`/
//! `TextTheme`, `ThemeData`, and the `Theme` inherited widget land in the
//! same crate as a follow-up commit.
//!
//! ## Flutter parity
//!
//! `package:flutter/material.dart`'s theming surface — primarily
//! `material/color_scheme.dart` and `material/theme_data.dart` (oracle tag
//! `3.44.0`) for this slice. Every constant table (`ColorScheme::light`/
//! `dark`) is a verbatim, per-value-cited port — see [`color_scheme`]
//! module docs for the exact oracle source.
//!
//! ## Scope (V1 — constants-first)
//!
//! This crate ships the fixed M3 baseline, not
//! [`ColorScheme::fromSeed`](https://api.flutter.dev/flutter/material/ColorScheme/ColorScheme.fromSeed.html)
//! (dynamic-color generation from a seed) — see [`color_scheme`] module
//! docs for the deferral rationale.
//!
//! ## Example
//!
//! ```rust
//! use flui_material::ColorScheme;
//!
//! let dark = ColorScheme::dark();
//! assert_eq!(dark.brightness, flui_types::platform::Brightness::Dark);
//! ```

#![deny(missing_docs)]

pub mod color_scheme;

pub use color_scheme::{ColorScheme, ColorSchemeOverrides};
