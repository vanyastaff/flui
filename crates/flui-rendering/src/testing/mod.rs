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
//! use flui_rendering::testing::{RenderTester, Probe, box_node};
//! use flui_rendering::prelude::*;
//! use flui_tree::Leaf;
//! use flui_types::{Size, geometry::px};
//!
//! // A minimal leaf render object used only to exercise the harness API.
//! // Concrete objects live in `flui_objects`; the harness itself is object-agnostic.
//! #[derive(Debug, Default)]
//! struct FixedBox;
//! impl flui_foundation::Diagnosticable for FixedBox {}
//! impl RenderBox for FixedBox {
//!     type Arity = Leaf;
//!     type ParentData = BoxParentData;
//!     fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
//!         Size::new(px(40.0), px(40.0))
//!     }
//!     fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
//! }
//!
//! let run = RenderTester::mount(box_node(FixedBox).label("root"))
//!     .run_frame();
//!
//! let root = run.id("root");
//! assert_eq!(run.box_geometry(root), Size::new(px(40.0), px(40.0)));
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
pub use harness::{
    CompositingRun,
    FrameRun,
    LayoutRun,
    PaintRun,
    RenderTester,
    SemanticsRun,
    // `has_overflow` moved to `flui-objects/tests/helpers.rs` (downcasts concrete objects).
};
pub use inspect::{Probe, hit_path_with_transforms, localize_hit_point};
pub use parent_data::ParentDataSeed;
pub use queries::BoxQueryRun;
pub use report::FrameReport;
// Primary snapshot API.
pub use snapshot::{
    SnapshotStrategy, assert_paints_node, is_draw_command_with_rect, is_draw_command_with_shadow,
    scene_diagnostics, scene_diagnostics_tree,
};
// Deprecated shims kept for one release cycle so external callers compile with
// a deprecation warning rather than a compile error. The allow-deprecated
// suppresses the use-of-deprecated lint at the re-export site; consumers that
// import these names will still receive the deprecation warning in their crate.
#[allow(deprecated)]
pub use snapshot::{
    // DrawCommandSummary predicate API (retired in favour of DiagnosticsNode
    // predicates with assert_paints_node).
    DrawCommandSummary,
    DrawKind,
    // Free-function snapshot helpers (retired in favour of scene_diagnostics /
    // assert_paints_node).
    assert_any,
    collect_commands,
    commands_of,
    // Layer-tree string serializers (retired in favour of scene_diagnostics +
    // SnapshotStrategy).
    serialize_layer_subtree,
    serialize_layer_tree,
    snapshot_subtree,
    snapshot_tree,
};
pub use tree::{
    RenderLabelRegistry, TreeNode, box_node, box_node_boxed, sliver_node, sliver_node_boxed,
};

// Harness self-tests were moved to `tests/harness_self_test.rs` (integration
// test) after the flui-objects extraction (ADR-0008). Internal lib unit tests
// cannot import from `flui_objects` without creating a duplicate-crate-version
// error (flui-objects has a production dep on flui-rendering; the lib-under-test
// and flui-objects' copy of flui-rendering are distinct compiled artifacts).
// Integration tests link the already-built library and do not have this problem.
