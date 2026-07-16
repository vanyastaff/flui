//! # `flui_cupertino`
//!
//! iOS-style (Cupertino) theming foundation for FLUI: [`CupertinoColors`] and
//! [`CupertinoDynamicColor`] (the brightness/contrast/elevation-adaptive
//! color system), [`CupertinoTextThemeData`], [`CupertinoThemeData`], the
//! [`CupertinoTheme`] inherited widget that publishes it to a subtree, and
//! [`CupertinoButton`].
//!
//! ## Flutter parity
//!
//! `package:flutter/cupertino.dart`'s theming surface — primarily
//! `cupertino/colors.dart`, `cupertino/text_theme.dart`, `cupertino/theme.dart`,
//! `cupertino/constants.dart`, and `cupertino/button.dart` (oracle tag
//! `3.44.0`). Every constant table (`CupertinoColors`' 30-odd dynamic-color
//! statics, `_kDefaultTextStyle` and its siblings, `_kDefaultTheme`, the
//! per-[`CupertinoButtonSize`] geometry maps in `constants.dart`) is a
//! verbatim, per-value-cited port — see each module's docs for the exact
//! oracle source.
//!
//! ## ADR-0028: no upward or sideways design-system dependency
//!
//! This crate depends only on `flui-widgets`/`flui-view`/`flui-types`/
//! `flui-animation` (plus `tracing`) — **never** `flui-material` (the
//! sibling design system; the two are independent, per ADR-0028) and
//! **never** `flui-objects`/`flui-rendering` directly (v1 composes existing
//! `flui-widgets` widgets — `FadeTransition`, `GestureDetector`,
//! `DecoratedBox`, … — it does not paint its own render objects).
//! `flui-foundation`/`flui-interaction` appear only as dev-dependencies,
//! for the `tests/common` mount harness — no `src/` code references either.
//!
//! ## Scope (V1 — constants-first, one component)
//!
//! This crate ships the color/typography/theme substrate plus one component,
//! [`CupertinoButton`], built on it. It deliberately does **not** ship:
//!
//! - Every other Cupertino component (`CupertinoNavigationBar`,
//!   `CupertinoTabScaffold`, `CupertinoTextField`, pickers, action sheets,
//!   `CupertinoPageRoute`'s swipe-back transition, …) — later increments on
//!   this same substrate.
//! - The contrast and interface-elevation axes of [`CupertinoDynamicColor`]
//!   resolution — both are stored (all 8 variants of every color are ported
//!   verbatim) but resolution always treats them as "normal contrast, base
//!   elevation": there is no `MediaQuery::high_contrast` field or
//!   `CupertinoUserInterfaceLevel` ambient in FLUI yet to resolve them
//!   against. Only the brightness axis (`CupertinoTheme` ambient ??
//!   `MediaQuery::platform_brightness`) is full oracle parity. See
//!   [`colors`] module docs.
//! - [`CupertinoButton`]'s focus ring (`RoundedSuperellipseBorder`
//!   outline — `flui-painting` has the primitive, this crate does not draw
//!   it yet), `WidgetState`-resolved mouse cursor, and
//!   `onFocusChange`/`autofocus` wiring. See [`button`] module docs.
//! - A `CupertinoApp` widget — this crate is the theming substrate a future
//!   `CupertinoApp` (or a plain `CupertinoTheme` at the app root) builds on,
//!   not the app scaffold itself.
//!
//! Each deferral is named, not silently dropped — see the owning module's
//! docs for the tracking rationale.
//!
//! ## Example
//!
//! ```rust
//! use flui_cupertino::{CupertinoTheme, CupertinoThemeData};
//! use flui_widgets::SizedBox;
//!
//! let _themed = CupertinoTheme::new(CupertinoThemeData::default(), SizedBox::shrink());
//! ```

#![deny(missing_docs)]

pub mod button;
pub mod colors;
pub mod text_theme;
pub mod theme;

pub use button::{CupertinoButton, CupertinoButtonSize, CupertinoButtonState};
pub use colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
pub use text_theme::CupertinoTextThemeData;
pub use theme::{CupertinoTheme, CupertinoThemeData};
