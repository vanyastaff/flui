//! FLUI — a Flutter-inspired declarative UI framework for Rust with a
//! `wgpu`-backed GPU rendering engine.
//!
//! This crate is the **facade** over the FLUI workspace: it re-exports the
//! layered crates an application author needs, so a downstream consumer can
//! depend on `flui` alone (by path — FLUI is pre-release and not on
//! crates.io) instead of naming each layer.
//!
//! # Quick start
//!
//! ```no_run
//! use flui::prelude::*;
//!
//! #[derive(Clone, StatelessView)]
//! struct Hello;
//!
//! impl StatelessView for Hello {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Container::new()
//!             .color(Color::rgb(18, 18, 24))
//!             .child(Center::new().child(Text::new("Hello, FLUI!")))
//!     }
//! }
//!
//! fn main() {
//!     flui::run_app(Hello);
//! }
//! ```
//!
//! # Choosing a catalog
//!
//! The base surface — [`widgets`], [`view`], [`animation`], [`run_app`], and
//! the non-Material half of [`prelude`] — needs no feature at all. The design
//! systems and the global localization implementations are feature-selected:
//!
//! | Feature | Default | Enables |
//! |---|---|---|
//! | `material` | **on** | `flui::material` and the Material half of [`prelude`] |
//! | `cupertino` | off | `flui::cupertino` |
//! | `localizations` | off | `flui::localizations` |
//! | `hot-reload` | off | desktop/Android development reload machinery inside [`app`] |
//!
//! `default = ["material"]` keeps the documented Material-first quick start
//! working out of the box. Turning defaults off (`default-features = false`)
//! gives a catalog-free application that still has the full widget layer,
//! navigation, focus, and media information. A module whose feature is off is
//! **absent**, not empty — `flui::cupertino` without the `cupertino` feature is
//! an unresolved-import error at the use site, which is the diagnostic you
//! want.
//! Web and iOS do not yet install reload drivers. The additive `hot-reload`
//! feature remains compile-safe on those targets so workspace feature
//! unification cannot break an otherwise supported cross-target build.
//!
//! # Using a design system
//!
//! [`prelude`] curates the everyday `flui-widgets` surface plus — when the
//! `material` feature is on — the common Material widgets (see [`prelude`]'s
//! own docs for the curation rule); the full catalogs live at `flui::material`
//! and `flui::cupertino`:
//!
#![cfg_attr(
    feature = "material",
    doc = r#"```no_run
use flui::material::{AppBar, Scaffold, Theme, ThemeData};
use flui::prelude::*;

#[derive(Clone, StatelessView)]
struct MaterialHello;

impl StatelessView for MaterialHello {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(
            ThemeData::light(),
            Scaffold::new()
                .app_bar(AppBar::new().title(Text::new("FLUI")))
                .body(Center::new().child(Text::new("Hello, Material!"))),
        )
    }
}

fn main() {
    flui::run_app(MaterialHello);
}
```"#
)]
//!
//! # Layers
//!
//! Each re-exported module is one workspace crate; the layering (no upward
//! edges) is documented in `docs/FOUNDATIONS.md`:
//!
//! | Module | Crate | Feature | Layer |
//! |---|---|---|---|
//! | [`types`] | `flui-types` | — | foundation types + unit system |
//! | [`geometry`] | `flui-geometry` | — | geometry primitives |
//! | [`foundation`] | `flui-foundation` | — | keys, listenables, diagnostics |
//! | [`view`] | `flui-view` | — | View/Element tree |
//! | [`widgets`] | `flui-widgets` | — | user-facing widget catalog |
//! | [`animation`] | `flui-animation` | — | curves, tweens, tickers |
//! | `material` | `flui-material` | `material` | Material Design theming + widget catalog |
//! | `cupertino` | `flui-cupertino` | `cupertino` | iOS-style theming + widget catalog |
//! | `localizations` | `flui-localizations` | `localizations` | global (multi-language) localized resources |
//! | [`app`] | `flui-app` | — | `run_app` + bindings |
//!
//! [`painting`], [`rendering`], and [`interaction`] expose selected authoring
//! contracts for custom drawing, render objects, and gestures. Arena storage,
//! the engine, and platform implementations remain outside this facade.
//! Enable `testing` in a development dependency for deterministic headless tests.
//! `flui::material` and `flui::cupertino` sit *above* [`widgets`] (ADR-0028's
//! design-system decoupling contract — `material --> widgets`,
//! `cupertino --> widgets`, never the reverse), which is why `flui` is on
//! that ADR's allowlist of crates permitted to depend on both: the facade is
//! the app-level aggregation point, not a core crate. `flui::localizations`
//! sits above both, implementing the catalogs' delegate contracts; the
//! catalogs never depend back on it.

// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub mod interaction;
pub mod painting;
pub mod rendering;
#[cfg(feature = "testing")]
pub mod testing;

pub use flui_animation as animation;
pub use flui_app as app;
/// The iOS-style design system (`flui-cupertino`). Requires the `cupertino`
/// feature.
#[cfg(feature = "cupertino")]
pub use flui_cupertino as cupertino;
pub use flui_foundation as foundation;
/// Structured diagnostic properties for application-defined types.
pub use flui_foundation::Diagnosticable;
pub use flui_geometry as geometry;
/// Development hot-reload support. Requires the `hot-reload` feature.
#[cfg(feature = "hot-reload")]
pub use flui_hot_reload as hot_reload;
/// Global (multi-language) implementations of the catalogs' localization
/// contracts (`flui-localizations`) — FLUI's analog of Flutter's
/// `flutter_localizations`. Requires the `localizations` feature.
#[cfg(feature = "localizations")]
pub use flui_localizations as localizations;
/// Derive structured diagnostic properties without a direct implementation-crate dependency.
pub use flui_macros::Diagnosticable;
/// The Material Design system (`flui-material`). Requires the `material`
/// feature, which is on by default.
#[cfg(feature = "material")]
pub use flui_material as material;
pub use flui_types as types;
pub use flui_view as view;
pub use flui_widgets as widgets;

/// The `android-activity` crate `android_main` receives its `AndroidApp`
/// from, so an application declares no Android dependency of its own.
#[cfg(target_os = "android")]
pub use flui_app::android_activity;
/// Application configuration. Re-exported from [`app`] (`flui-app`).
pub use flui_app::app::AppConfig;
/// The errors a window request can fail with — what [`open_window`] and
/// [`AppHandle`] report, and what an [`Application`] window-error observer
/// receives. Re-exported from [`app`] (`flui-app`); desktop only, like the
/// entry points that produce it.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::AppWindowError;
/// Whether an additional window joins the caller's realm or gets its own.
/// Re-exported from [`app`] (`flui-app`); absent on iOS, where no
/// secondary-window entry point exists.
#[cfg(not(target_os = "ios"))]
pub use flui_app::app::WindowPolicy;
/// Open an additional top-level window without widget content.
/// Re-exported from [`app`] (`flui-app`).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::open_secondary_window;
/// Open an additional top-level window with mounted widget content.
/// Re-exported from [`app`] (`flui-app`).
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::open_window;
/// The resident-application builder and its control surface: a reusable
/// main-window factory, windowless startup, and a `Send + Sync` handle that
/// can show the window or quit from any thread. Re-exported from [`app`]
/// (`flui-app`); desktop only.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use flui_app::app::{AppControlError, AppHandle, AppRunError, Application, StartupWindow};
/// The application entry point — builds the tree, opens a window, and drives
/// the frame loop. Re-exported from [`app`] (`flui-app`).
pub use flui_app::run_app;
/// [`run_app`] with an explicit [`AppConfig`] (window title, size, services,
/// failure policy). Re-exported from [`app`] (`flui-app`).
pub use flui_app::run_app_with_config;
/// The Android entry points, called from the `cdylib`'s `android_main`
/// (`flui create` writes one). Re-exported from `flui-app`.
#[cfg(target_os = "android")]
pub use flui_app::{run_app_android, run_app_android_with_config};

/// Everything an application author needs in scope to write widget code:
/// the widget catalog prelude, [`run_app`], and — with the `material` feature
/// on — the everyday Material widgets.
///
/// # Two halves
///
/// The **base half** ([`flui_widgets::prelude`] plus [`run_app`]) is always
/// present. It is design-system-neutral, so an application that selects no
/// catalog still writes ordinary screens off `use flui::prelude::*`.
///
/// The **Material half** is `#[cfg(feature = "material")]`. Turning the
/// feature off removes those names from the glob rather than replacing them
/// with stubs; a build that was relying on them fails at the use site.
///
/// # Curation rule (Material half)
///
/// This module answers "what does *every* app touch" — not "what does
/// `flui-material` export." A type belongs here when an app author reaches
/// for it while writing ordinary screens: layout/text/interaction primitives
/// (via [`flui_widgets::prelude`]), theming (`Theme`, `ThemeData`,
/// `ColorScheme`, `TextTheme`), the app shell (`Scaffold`, `AppBar`,
/// `Drawer`), the standard button family, and the common
/// data-display/feedback/navigation widgets a Material screen composes
/// (`Card`, `ListTile`, `Dialog`, `SnackBar`, `NavigationBar`, the tab
/// family, …).
///
/// It does **not** include: `*State` handles an app never constructs
/// directly (`InkWellState`, `TabBarState`, …), component-theme override
/// structs (`AppBarThemeData` and friends — advanced per-widget
/// customization, reach them at `flui::material`), or `flui-material`
/// internals like the raw M3 type-scale table (`english_like_2021`).
///
/// **`TextField` is deliberately absent.** `flui-widgets` and
/// `flui-material` each ship a distinct type of that name — a design-agnostic
/// text-editing primitive and the M3-styled input — so a curated glob cannot
/// carry both without one silently shadowing the other. [`prelude`] keeps
/// [`flui_widgets::TextField`] (already part of [`flui_widgets::prelude`]);
/// reach the Material one explicitly as `flui::material::TextField`. The
/// Cupertino catalog has no such collision
/// (every type is `Cupertino`-prefixed), but its surface is app-shell-shaped
/// rather than everyday-widget-shaped (`CupertinoPageScaffold`,
/// `CupertinoTabScaffold`, …), so it stays at `flui::cupertino` rather than
/// joining this glob.
pub mod prelude {
    pub use flui_app::app::AppConfig;
    #[cfg(not(target_os = "ios"))]
    pub use flui_app::app::WindowPolicy;
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub use flui_app::app::{AppWindowError, open_secondary_window, open_window};
    pub use flui_app::{run_app, run_app_with_config};
    #[cfg(feature = "material")]
    pub use flui_material::{
        AlertDialog, AppBar, BackButton, Card, Checkbox, Chip, ColorScheme, DefaultTabController,
        Dialog, Divider, Drawer, ElevatedButton, FilledButton, FilterChip, FloatingActionButton,
        IconButton, InkWell, ListTile, Material, MaterialApp, NavigationBar, NavigationDestination,
        OutlinedButton, Radio, Scaffold, ScaffoldMessenger, ScaffoldMessengerHandle,
        ScaffoldMessengerScope, SnackBar, Switch, Tab, TabBar, TabBarView, TabController,
        TextButton, TextTheme, Theme, ThemeData, ThemeMode, VerticalDivider, show_dialog,
    };
    pub use flui_widgets::prelude::*;
}
