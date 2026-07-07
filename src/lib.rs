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
//! | [`app`] | `flui-app` | `run_app` + bindings |
//!
//! Lower layers (rendering, painting, engine, platform) are deliberately not
//! re-exported: their surfaces are consumed *through* the widget layer and
//! remain path-dependencies for the rare integrator who needs them directly.

// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]

pub use flui_animation as animation;
pub use flui_app as app;
pub use flui_foundation as foundation;
pub use flui_geometry as geometry;
pub use flui_types as types;
pub use flui_view as view;
pub use flui_widgets as widgets;

/// The application entry point — builds the tree, opens a window, and drives
/// the frame loop. Re-exported from [`app`] (`flui-app`).
pub use flui_app::run_app;

/// Everything an application author needs in scope to write widget code:
/// the widget catalog prelude plus [`run_app`].
pub mod prelude {
    pub use flui_app::run_app;
    pub use flui_widgets::prelude::*;
}
