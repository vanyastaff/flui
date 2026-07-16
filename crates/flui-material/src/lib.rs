//! # `flui_material`
//!
//! Material Design theming foundation for FLUI: [`ColorScheme`], the M3 2021
//! type scale ([`typography`]) and [`TextTheme`], [`ThemeData`], and the
//! [`Theme`] inherited widget that publishes it to a subtree.
//!
//! ## Flutter parity
//!
//! `package:flutter/material.dart`'s theming surface — primarily
//! `material/color_scheme.dart`, `material/typography.dart`,
//! `material/text_theme.dart`, `material/theme_data.dart`, and
//! `material/theme.dart` (oracle tag `3.44.0`). Every constant table
//! (`ColorScheme::light`/`dark`, [`typography::english_like_2021`],
//! [`TextTheme::black_mountain_view`]/[`white_mountain_view`](TextTheme::white_mountain_view))
//! is a verbatim, per-value-cited port — see each module's docs for the exact
//! oracle source.
//!
//! ## Scope (V1 — constants-first)
//!
//! This crate ships the fixed M3 baseline: two literal color schemes, one
//! literal type scale, and the plumbing (`ThemeData`, `Theme`) to compose and
//! publish them. It deliberately does **not** ship:
//!
//! - [`ColorScheme::fromSeed`](https://api.flutter.dev/flutter/material/ColorScheme/ColorScheme.fromSeed.html)
//!   — dynamic-color generation from a seed. See [`color_scheme`] module docs.
//! - Component themes (`ButtonThemeData`, `InputDecorationTheme`, …) — these
//!   land with their owning components; [`ThemeData`] is `#[non_exhaustive]`
//!   to receive them without a breaking change.
//! - `AnimatedTheme` / `ColorScheme`/`TextTheme` lerp — no component consumes
//!   an interpolated theme yet.
//! - Dense/tall type-scale geometries (CJK / Farsi-Hindi-Thai) — only
//!   `englishLike2021` is ported; see [`typography`] module docs.
//! - A `MaterialApp` widget — this crate is the theming substrate a future
//!   `MaterialApp` (or a plain `Theme` at the app root) builds on, not the
//!   app scaffold itself.
//!
//! Each deferral is named, not silently dropped — see the owning module's
//! docs for the tracking rationale.
//!
//! ## Example
//!
//! ```rust
//! use flui_material::{Theme, ThemeData};
//! use flui_widgets::SizedBox;
//!
//! let _themed = Theme::new(ThemeData::dark(), SizedBox::shrink());
//! ```

#![deny(missing_docs)]

pub mod color_scheme;
pub mod ink_well;
pub mod material;
pub mod shape;
pub mod text_theme;
pub mod theme;
pub mod theme_data;
pub mod typography;

pub use color_scheme::{ColorScheme, ColorSchemeOverrides};
pub use ink_well::{InkWell, InkWellState};
pub use material::Material;
pub use shape::MaterialShape;
pub use text_theme::TextTheme;
pub use theme::Theme;
pub use theme_data::{ThemeData, ThemeDataOverrides};
pub use typography::english_like_2021;
