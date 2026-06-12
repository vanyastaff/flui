//! Layer-tree test harness.
//!
//! Full API reference and examples: `crates/flui-layer/docs/TESTING.md`.
//!
//! Compiled only for this crate's own tests (`cfg(test)`) or when a consumer
//! enables the `testing` feature. Provides:
//!
//! - a declarative [`LayerTree`](crate::LayerTree) builder ([`layer`],
//!   [`LayerSpec`]);
//! - a [`LayerTester`] wrapper with structural / bounds / diagnostics
//!   inspection (mirrors `flui_rendering::testing::RenderTester`);
//! - free [`inspect`] functions (`structure`, `first_picture_bounds`,
//!   `diagnostics_tree`) that are the single source of truth `flui-rendering`
//!   re-uses.
//!
//! # Example
//!
//! ```
//! use flui_layer::testing::{LayerTester, layer};
//! use flui_layer::{CanvasLayer, OffsetLayer};
//! use flui_types::geometry::px;
//!
//! let probe = LayerTester::mount(
//!     layer(OffsetLayer::new(flui_types::Offset::new(px(5.0), px(5.0))))
//!         .child(layer(CanvasLayer::new()).label("canvas")),
//! );
//! assert_eq!(probe.structure(), vec!["Offset", "Canvas"]);
//! assert_eq!(probe.kind(probe.id("canvas")), "Canvas");
//! ```

pub mod inspect;
mod spec;
mod tester;

pub use spec::{layer, mount, LayerLabelRegistry, LayerSpec};
pub use tester::LayerTester;

#[cfg(test)]
mod tests;
