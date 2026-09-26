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
//!   [`types`], [`view`] and [`widgets`] are the internal crates themselves,
//!   so `flui_sdk::widgets::Text` and `flui::widgets::Text` are one type.
//! - **Curated modules** at the facade's paths: [`interaction`], [`painting`]
//!   and [`rendering`] hold the subset of the facade's module that packages
//!   use, as the same items, not wrappers.
//! - **Evolving module** [`pipeline`]: render-object internals the facade does
//!   not expose. A package's exposure to it is
//!   `grep 'flui_sdk::pipeline'`.
//!
//! Each item is here because `flui-material` or `flui-cupertino` imports it
//! outside its tests; `tests/surface.rs` pins the list.

pub use flui_animation as animation;
pub use flui_foundation as foundation;
pub use flui_types as types;
pub use flui_view as view;
pub use flui_widgets as widgets;

/// Gesture details and focus, at the paths `flui::interaction` uses.
pub mod interaction {
    pub use flui_interaction::DragDownDetails;
    pub use flui_interaction::routing::FocusNode;
}

/// Custom painting, at the paths `flui::painting` uses.
///
/// `DrawOp` is here for packages' paint tests, which read back the recorded
/// operations; no package names it outside its tests.
pub mod painting {
    pub use flui_painting::Canvas;
    pub use flui_painting::DrawOp;
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
