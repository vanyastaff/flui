//! # `flui_material`
//!
//! Material Design theming foundation for FLUI: [`ColorScheme`], the M3 2021
//! type scale ([`typography`]) and [`TextTheme`], [`ThemeData`], the
//! [`Theme`] inherited widget that publishes it to a subtree, the
//! [`Material`]/[`InkWell`] surface primitives, the M3 button family
//! ([`ButtonStyle`], [`ElevatedButton`], [`FilledButton`], [`OutlinedButton`],
//! [`TextButton`], [`IconButton`], [`FloatingActionButton`], [`BackButton`]),
//! [`Scaffold`]/[`AppBar`], [`Card`], [`Dialog`]/[`AlertDialog`]/
//! [`show_dialog`], the input decoration substrate ([`InputDecoration`],
//! [`InputDecorator`], [`TextField`]), and the data-display family
//! ([`ListTile`], [`Divider`]/[`VerticalDivider`]).
//!
//! ## Flutter parity
//!
//! `package:flutter/material.dart`'s theming surface — primarily
//! `material/color_scheme.dart`, `material/typography.dart`,
//! `material/text_theme.dart`, `material/theme_data.dart`,
//! `material/theme.dart`, `material/material.dart`, `material/ink_well.dart`,
//! `material/button_style.dart`, `material/button_style_button.dart`, and the
//! four concrete button files (`elevated_button.dart`, `filled_button.dart`,
//! `outlined_button.dart`, `text_button.dart`) (oracle tag `3.44.0`). Every
//! constant table (`ColorScheme::light`/`dark`,
//! [`typography::english_like_2021`],
//! [`TextTheme::black_mountain_view`]/[`white_mountain_view`](TextTheme::white_mountain_view),
//! each button's `_TokenDefaultsM3`) is a verbatim, per-value-cited port —
//! see each module's docs for the exact oracle source.
//!
//! ## Scope (V1 — constants-first)
//!
//! This crate ships the fixed M3 baseline: two literal color schemes, one
//! literal type scale, and the plumbing (`ThemeData`, `Theme`) to compose and
//! publish them. It deliberately does **not** ship:
//!
//! - [`ColorScheme::fromSeed`](https://api.flutter.dev/flutter/material/ColorScheme/ColorScheme.fromSeed.html)
//!   — dynamic-color generation from a seed. See [`color_scheme`] module docs.
//! - Every shipped widget's own component theme slot (`ElevatedButtonThemeData`
//!   and friends, [`AppBarThemeData`], [`CardThemeData`], [`DialogThemeData`],
//!   [`FabThemeData`], [`InputDecorationThemeData`]) is narrowed to the
//!   fields its owning widget actually consumes — see `theme_data`'s module
//!   docs and each type's own doc comment for the ported/deferred field
//!   split. [`ThemeData`] stays `#[non_exhaustive]` to receive the rest
//!   without a breaking change.
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

pub mod app_bar;
pub mod back_button;
pub mod button_style;
mod button_style_button;
pub mod card;
pub mod color_scheme;
pub mod dialog;
pub mod divider;
pub mod elevated_button;
pub mod filled_button;
pub mod floating_action_button;
pub mod icon_button;
pub mod ink_well;
pub mod input_decorator;
pub mod list_tile;
pub mod material;
pub mod outlined_button;
pub mod scaffold;
pub mod shape;
pub mod text_button;
pub mod text_field;
pub mod text_theme;
pub mod theme;
pub mod theme_data;
pub mod typography;

pub use app_bar::AppBar;
pub use back_button::BackButton;
pub use button_style::ButtonStyle;
pub use card::Card;
pub use color_scheme::{ColorScheme, ColorSchemeOverrides};
pub use dialog::{AlertDialog, Dialog, show_dialog};
pub use divider::{Divider, VerticalDivider};
pub use elevated_button::ElevatedButton;
pub use filled_button::FilledButton;
pub use floating_action_button::FloatingActionButton;
pub use icon_button::IconButton;
pub use ink_well::{InkWell, InkWellState};
pub use input_decorator::{InputDecoration, InputDecorator, InputDecoratorState};
pub use list_tile::ListTile;
pub use material::Material;
pub use outlined_button::OutlinedButton;
pub use scaffold::Scaffold;
pub use shape::MaterialShape;
pub use text_button::TextButton;
pub use text_field::{TextField, TextFieldState};
pub use text_theme::TextTheme;
pub use theme::Theme;
pub use theme_data::{
    AppBarThemeData, CardThemeData, DialogThemeData, DividerThemeData, ElevatedButtonThemeData,
    FabThemeData, FilledButtonThemeData, IconButtonThemeData, InputDecorationThemeData,
    ListTileThemeData, OutlinedButtonThemeData, TextButtonThemeData, ThemeData, ThemeDataOverrides,
};
pub use typography::english_like_2021;
