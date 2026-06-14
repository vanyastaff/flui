//! Render-object test harness — build real trees, drive the production
//! pipeline, inspect the result.
//!
//! Full API reference and examples: `crates/flui-rendering/docs/TESTING.md`.
//!
//! This module is compiled only for this crate's own tests (`cfg(test)`) or
//! when a consumer enables the `testing` feature. It exists so render
//! objects can be exercised through the **real** [`PipelineOwner`] pipeline
//! (not mocks), with one ergonomic, protocol-agnostic surface.
//!
//! # Shape
//!
//! 1. Describe a tree with [`box_node`] / [`sliver_node`], nesting children,
//!    tagging nodes with [`TreeNode::label`], and optionally attaching
//!    [`ParentDataSeed`] presets (stack positioning, flex factors, …) via
//!    [`TreeNode::with_parent_data_seed`].
//! 2. Hand it to [`RenderTester::mount`] and pick a run depth — both apply
//!    equally to Box and Sliver:
//!    - [`RenderTester::run_layout`] -> [`LayoutRun`] (geometry/offsets,
//!      no frame);
//!    - [`RenderTester::run_frame`] -> [`FrameRun`] (full frame: layer
//!      structure, picture bounds, [`FrameReport`], repeated `pump`).
//! 3. Inspect via the shared [`Probe`] trait (`offset`, `box_geometry`,
//!    `sliver_geometry`, `hit`, `id`) and, for proxy/leaf query contracts,
//!    [`BoxQueryRun`] (`min_intrinsic_width`, `dry_layout`, `dry_baseline`, …).
//! 4. Drive multi-frame / animation scenarios on [`FrameRun`]:
//!    [`FrameRun::advance_layout`] / [`FrameRun::advance_paint`] (mutate +
//!    one frame), [`FrameRun::simulate`] (tick loop + pump per step),
//!    [`FrameRun::pump_frames`] / [`FrameRun::pump_idle_frames`] (skip settled
//!    frames). Layout changes use [`FrameRun::update`]; paint-only changes use
//!    [`FrameRun::update_paint`] (Box and Sliver alike).
//!
//! [`PipelineOwner`]: crate::pipeline::PipelineOwner
//!
//! # Example
//!
//! ```
//! use flui_rendering::objects::{RenderColoredBox, RenderPadding};
//! use flui_rendering::testing::{RenderTester, Probe, box_node};
//! use flui_types::{Offset, Size, geometry::px};
//!
//! let run = RenderTester::mount(
//!     box_node(RenderPadding::all(5.0))
//!         .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
//! )
//! .with_size(Size::new(px(200.0), px(200.0)))
//! .run_frame();
//!
//! let child = run.id("child");
//! assert_eq!(run.offset(child), Offset::new(px(5.0), px(5.0)));
//! assert!(run.painted());
//! ```

pub mod assertions;
mod harness;
pub mod inspect;
pub mod parent_data;
pub mod queries;
mod report;
pub mod sliver;
pub mod snapshot;
pub mod tree;

pub use assertions::{
    assert_descendant_properties, assert_has_committed_geometry, assert_has_committed_size,
    assert_properties,
};
pub use harness::{FrameRun, LayoutRun, RenderTester};
pub use inspect::Probe;
pub use parent_data::ParentDataSeed;
pub use queries::BoxQueryRun;
pub use report::FrameReport;
pub use snapshot::{
    DrawCommandSummary, DrawKind, collect_commands, serialize_layer_subtree, serialize_layer_tree,
    summarize_command,
};
pub use tree::{
    RenderLabelRegistry, TreeNode, box_node, box_node_boxed, sliver_node, sliver_node_boxed,
};

#[cfg(test)]
mod tests;
