//! # `flui_cupertino`
//!
//! iOS-style (Cupertino) theming foundation and component set for FLUI:
//! [`CupertinoColors`] and [`CupertinoDynamicColor`] (the
//! brightness/contrast/elevation-adaptive color system),
//! [`CupertinoTextThemeData`], [`CupertinoThemeData`], the [`CupertinoTheme`]
//! inherited widget that publishes it to a subtree, [`CupertinoButton`],
//! [`cupertino_page_route`] (the iOS slide-in page transition with
//! edge-swipe-back), [`CupertinoNavigationBar`], [`CupertinoPageScaffold`],
//! and [`CupertinoTabScaffold`]/[`CupertinoTabBar`] (the lazy-build,
//! state-retaining tabbed root layout).
//!
//! ## Flutter parity
//!
//! `package:flutter/cupertino.dart`'s theming and single-page/tabbed-app
//! surface — primarily `cupertino/colors.dart`, `cupertino/text_theme.dart`,
//! `cupertino/theme.dart`, `cupertino/constants.dart`, `cupertino/button.dart`,
//! `cupertino/route.dart`, `cupertino/nav_bar.dart`, `cupertino/page_scaffold.dart`,
//! `cupertino/tab_scaffold.dart`, and `cupertino/bottom_tab_bar.dart` (oracle
//! tag `3.44.0`). Every constant table (`CupertinoColors`' 30-odd
//! dynamic-color statics, `_kDefaultTextStyle` and its siblings,
//! `_kDefaultTheme`, the per-[`CupertinoButtonSize`] geometry maps in
//! `constants.dart`, `_kNavBarPersistentHeight`, `_kTabBarHeight`, the
//! nav-bar/tab-bar border colors) is a verbatim, per-value-cited port — see
//! each module's docs for the exact oracle source.
//!
//! ## ADR-0028: no upward or sideways design-system dependency
//!
//! This crate depends only on `flui-widgets`/`flui-view`/`flui-types`/
//! `flui-animation`/`flui-objects`/`flui-foundation` (plus `tracing`) —
//! **never** `flui-material` (the sibling design system; the two are
//! independent, per ADR-0028) and **never** a `flui-objects`/`flui-rendering`
//! render object of its own (every component here composes existing
//! `flui-widgets` widgets — `FadeTransition`, `GestureDetector`,
//! `DecoratedBox`, `SlideTransition`, `Stack`/`Positioned`, `Offstage`,
//! `HeroMode`, … — it does not paint its own render objects). The
//! `flui-objects` dependency is exactly one value type
//! ([`flui_objects::TranslationFraction`]) that `flui-widgets`' own
//! `SlideTransition` requires as a parameter (see [`route`]'s module docs);
//! the `flui-foundation` dependency is [`CupertinoTabController`]'s notify
//! substrate (`ChangeNotifier`/`Listenable`, see [`tab_scaffold`]'s module
//! docs) — both value-type/trait dependencies, not render objects. See each
//! dependency's `Cargo.toml` comment. `flui-interaction` appears only as a
//! dev-dependency, for the `tests/common` mount harness — no `src/` code
//! references it.
//!
//! ## Scope (V1 — theming, one page route, and the two-scaffold family)
//!
//! This crate ships the color/typography/theme substrate,
//! [`CupertinoButton`], [`cupertino_page_route`], [`CupertinoNavigationBar`],
//! [`CupertinoPageScaffold`], and [`CupertinoTabScaffold`]/[`CupertinoTabBar`].
//! It deliberately does **not** ship:
//!
//! - Every other Cupertino component not listed above (`CupertinoTextField`,
//!   pickers, action sheets, `CupertinoSliverNavigationBar`/large titles, …)
//!   — later increments on this same substrate.
//! - `cupertino_page_route`'s edge shadow, `title`/`previousTitle`,
//!   `fullscreenDialog`, and `delegatedTransition` — see [`route`]'s module
//!   docs for exactly what and why.
//! - `CupertinoNavigationBar`'s `.large()`/large-title layout, automatic
//!   leading/middle heuristics, scroll-under background fade, blur, and the
//!   hero transition between nav bars across a route push — see [`nav_bar`]'s
//!   module docs.
//! - `CupertinoTabBar`'s blur on a translucent background, and state
//!   restoration for `CupertinoTabScaffold` — see [`bottom_tab_bar`]'s and
//!   [`tab_scaffold`]'s module docs.
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

pub mod bottom_tab_bar;
pub mod button;
pub mod colors;
pub mod nav_bar;
pub mod page_scaffold;
pub mod route;
pub mod tab_scaffold;
pub mod text_theme;
pub mod theme;

pub use bottom_tab_bar::{CupertinoTabBar, CupertinoTabBarItem};
pub use button::{CupertinoButton, CupertinoButtonSize, CupertinoButtonState};
pub use colors::{CupertinoColor, CupertinoColors, CupertinoDynamicColor};
pub use nav_bar::CupertinoNavigationBar;
pub use page_scaffold::CupertinoPageScaffold;
pub use route::cupertino_page_route;
pub use tab_scaffold::{CupertinoTabController, CupertinoTabScaffold, CupertinoTabScaffoldState};
pub use text_theme::CupertinoTextThemeData;
pub use theme::{CupertinoTheme, CupertinoThemeData};
