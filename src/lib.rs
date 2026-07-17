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
//! # Using a design system
//!
//! [`prelude`] curates the everyday `flui-widgets` surface plus the common
//! Material widgets (see [`prelude`]'s own docs for the curation rule); the
//! full catalogs live at [`material`] and [`cupertino`]:
//!
//! ```no_run
//! use flui::material::{AppBar, Scaffold, Theme, ThemeData};
//! use flui::prelude::*;
//!
//! #[derive(Clone, StatelessView)]
//! struct MaterialHello;
//!
//! impl StatelessView for MaterialHello {
//!     fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
//!         Theme::new(
//!             ThemeData::light(),
//!             Scaffold::new()
//!                 .app_bar(AppBar::new().title(Text::new("FLUI")))
//!                 .body(Center::new().child(Text::new("Hello, Material!"))),
//!         )
//!     }
//! }
//!
//! fn main() {
//!     flui::run_app(MaterialHello);
//! }
//! ```
//!
//! # Layers
//!
//! Each re-exported module is one workspace crate; the layering (no upward
//! edges) is documented in `docs/FOUNDATIONS.md`:
//!
//! | Module | Crate | Layer |
//! |---|---|---|
//! | [`types`] | `flui-types` | foundation types + unit system |
//! | [`geometry`] | `flui-geometry` | geometry primitives |
//! | [`foundation`] | `flui-foundation` | keys, listenables, diagnostics |
//! | [`view`] | `flui-view` | View/Element tree |
//! | [`widgets`] | `flui-widgets` | user-facing widget catalog |
//! | [`animation`] | `flui-animation` | curves, tweens, tickers |
//! | [`material`] | `flui-material` | Material Design theming + widget catalog |
//! | [`cupertino`] | `flui-cupertino` | iOS-style theming + widget catalog |
//! | [`app`] | `flui-app` | `run_app` + bindings |
//!
//! Lower layers (rendering, painting, engine, platform) are deliberately not
//! re-exported: their surfaces are consumed *through* the widget layer and
//! remain path-dependencies for the rare integrator who needs them directly.
//! [`material`] and [`cupertino`] sit *above* [`widgets`] (ADR-0028's
//! design-system decoupling contract — `material --> widgets`,
//! `cupertino --> widgets`, never the reverse), which is why `flui` is on
//! that ADR's allowlist of crates permitted to depend on both: the facade is
//! the app-level aggregation point, not a core crate.

// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub use flui_animation as animation;
pub use flui_app as app;
pub use flui_cupertino as cupertino;
pub use flui_foundation as foundation;
pub use flui_geometry as geometry;
pub use flui_material as material;
pub use flui_types as types;
pub use flui_view as view;
pub use flui_widgets as widgets;

/// The application entry point — builds the tree, opens a window, and drives
/// the frame loop. Re-exported from [`app`] (`flui-app`).
pub use flui_app::run_app;

/// Everything an application author needs in scope to write widget code:
/// the widget catalog prelude plus the everyday Material widgets, plus
/// [`run_app`].
///
/// # Curation rule
///
/// This module answers "what does *every* app touch" — not "what does
/// `flui-material` export." A type belongs here when an app author reaches
/// for it while writing ordinary screens: layout/text/interaction primitives
/// (via [`flui_widgets::prelude`]), theming ([`material::Theme`],
/// [`material::ThemeData`], [`material::ColorScheme`], [`material::TextTheme`]),
/// the app shell ([`material::Scaffold`], [`material::AppBar`],
/// [`material::Drawer`]), the standard button family, and the common
/// data-display/feedback/navigation widgets a Material screen composes
/// ([`material::Card`], [`material::ListTile`], [`material::Dialog`],
/// [`material::SnackBar`], [`material::NavigationBar`], the tab family, …).
///
/// It does **not** include: `*State` handles an app never constructs
/// directly (`InkWellState`, `TabBarState`, …), component-theme override
/// structs (`AppBarThemeData` and friends — advanced per-widget
/// customization, reach them at [`material`]), or `flui-material` internals
/// like the raw M3 type-scale table ([`material::english_like_2021`]).
///
/// **`TextField` is deliberately absent.** `flui-widgets` and
/// `flui-material` each ship a distinct type of that name — a design-agnostic
/// text-editing primitive and the M3-styled input — so a curated glob cannot
/// carry both without one silently shadowing the other. [`prelude`] keeps
/// [`flui_widgets::TextField`] (already part of [`flui_widgets::prelude`]);
/// reach the Material one explicitly as [`material::TextField`]. The
/// Cupertino catalog has no such collision
/// (every type is `Cupertino`-prefixed), but its surface is app-shell-shaped
/// rather than everyday-widget-shaped (`CupertinoPageScaffold`,
/// `CupertinoTabScaffold`, …), so it stays at [`cupertino`] rather than
/// joining this glob.
pub mod prelude {
    pub use flui_app::run_app;
    pub use flui_material::{
        AlertDialog, AppBar, BackButton, Card, Checkbox, Chip, ColorScheme, DefaultTabController,
        Dialog, Divider, Drawer, ElevatedButton, FilledButton, FilterChip, FloatingActionButton,
        IconButton, InkWell, ListTile, Material, NavigationBar, NavigationDestination,
        OutlinedButton, Radio, Scaffold, ScaffoldMessenger, ScaffoldMessengerHandle,
        ScaffoldMessengerScope, SnackBar, Switch, Tab, TabBar, TabBarView, TabController,
        TextButton, TextTheme, Theme, ThemeData, VerticalDivider, show_dialog,
    };
    pub use flui_widgets::prelude::*;
}
