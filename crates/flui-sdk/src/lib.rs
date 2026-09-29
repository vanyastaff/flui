//! The package-author surface of FLUI (ADR-0088 §4).
//!
//! A package such as a design system depends on this crate instead of on the
//! internal crates one by one, and it never pulls in the host, the engine or
//! the GPU stack: `flui-app`, `flui-engine` and `wgpu` are outside its normal
//! dependency closure, which `cargo xtask reach` checks.
//!
//! The crate is **Evolving**: it has its own `0.N` version, bumped on every
//! train, and its surface may change on any train without a `flui` major.
//!
//! - **Whole modules** at the facade's paths: [`animation`], [`foundation`],
//!   [`geometry`] and [`widgets`] are the internal crates (or, for
//!   `geometry`, foundation's module) themselves, so `flui_sdk::widgets::Text`
//!   and `flui::widgets::Text` are one type. [`view`] is a glob of the whole
//!   `flui-view` crate that shadows its `__runtime` seam (ADR-0081 §4).
//! - **Curated modules** at the facade's paths: [`interaction`], [`painting`],
//!   [`platform`] and [`rendering`] hold the subset of the facade's module that
//!   packages use, as the same items, not wrappers.
//! - **Evolving modules** [`pipeline`] and [`hooks`]: render-object internals
//!   and development hooks the facade does not expose. A package's exposure to
//!   them is `grep -E 'flui_sdk::(pipeline|hooks)::'`.
//!
//! Each item is here because `flui-material`, `flui-cupertino` or
//! `flui-devtools` imports it outside its tests; `tests/surface.rs` pins the
//! list.

pub use flui_animation as animation;
pub use flui_foundation as foundation;
pub use flui_foundation::geometry;
pub use flui_widgets as widgets;

/// The view layer (`flui-view`): every public item of the crate, at the
/// facade's path, as the same items.
///
/// A glob module rather than a whole-crate alias, so that it can shadow
/// `flui_view::__runtime`, the composition roots' seam (ADR-0081 §4), which
/// is not package-author surface:
///
/// ```compile_fail,E0603
/// use flui_sdk::view::__runtime::BindingRuntime;
/// ```
pub mod view {
    pub use flui_view::*;
    #[expect(
        hidden_glob_reexports,
        reason = "shadows the glob's `__runtime` (ADR-0081 §4): the composition roots' seam is not SDK surface"
    )]
    mod __runtime {}
}

/// Platform values: brightness and locale.
pub mod platform {
    pub use flui_platform_api::Brightness;
    pub use flui_platform_api::Locale;
}

/// Gesture details and focus, at the paths `flui::interaction` uses.
pub mod interaction {
    pub use flui_interaction::DragDownDetails;
    pub use flui_interaction::routing::FocusNode;
}

/// Custom painting and the paint, style and text values, at the paths `flui::painting` uses.
///
/// `DrawOp` is here for packages' paint tests, which read back the recorded
/// operations; no package names it outside its tests.
pub mod painting {
    pub use flui_painting::Alignment;
    pub use flui_painting::Canvas;
    pub use flui_painting::DrawOp;
    pub use flui_painting::paint::Clip;
    pub use flui_painting::paint::Paint;
    pub use flui_painting::paint::Path;
    pub use flui_painting::styling::Border;
    pub use flui_painting::styling::BorderRadius;
    pub use flui_painting::styling::BorderRadiusExt;
    pub use flui_painting::styling::BorderSide;
    pub use flui_painting::styling::BorderStyle;
    pub use flui_painting::styling::BoxDecoration;
    pub use flui_painting::styling::Color;
    pub use flui_painting::typography::FontWeight;
    pub use flui_painting::typography::TextDirection;
    pub use flui_painting::typography::TextStyle;
}

/// Render-object authoring, at the paths `flui::rendering` uses.
pub mod rendering {
    pub use flui_rendering::RenderUpdateImpact;
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::hit_testing::HitTestBehavior;
    pub use flui_rendering::protocol::BoxProtocol;
}

/// Render objects and their configuration that the facade does not expose.
///
/// **Evolving:** these are internals of the render-object catalog. They may
/// change or move on any train; an item graduates into a facade module once it
/// has stayed unchanged across trains and has a second consumer (ADR-0088 §4).
pub mod pipeline {
    pub use flui_objects::PathClipConfiguration;
    pub use flui_objects::RenderPhysicalShape;
    pub use flui_objects::TranslationFraction;
}

/// Development hooks the facade does not expose.
///
/// **Evolving** (ADR-0088 §4), like [`pipeline`]: frame telemetry that
/// development tooling reads. The tree-observation seam is not here; it is
/// `foundation::observe`, at the facade's path.
///
/// No public API produces a `FrameSnapshot` yet: the
/// presentation's frame clock is internal to the app host. A public snapshot
/// source is the follow-up recorded in ADR-0088's move 4.
pub mod hooks {
    pub use flui_scheduler::FrameSnapshot;
}
