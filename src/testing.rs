//! Deterministic application tests, available with the `testing` feature.
//!
//! Enable `flui/testing` in a consumer's development dependency. The optional
//! test driver is absent from ordinary application dependency graphs.
//! [`HeadlessBinding`] owns its clock and frame state; [`widgets`] provides
//! widget mounting, layout inspection, and pointer-input helpers.

pub use flui_testing::widgets;
pub use flui_testing::{BuildCapabilities, HeadlessBinding, MountOptions, MountOwners, Mounted};
pub use flui_testing::{a11y, replay};

/// Render-object tests that drive layout and queries without a widget tree,
/// plus the render-tree diagnostics dump a mounted application can be
/// inspected with ([`render_diagnostics`](rendering::render_diagnostics) over
/// [`HeadlessBinding::pipeline_owner`]).
/// [`collect_commands`](rendering::collect_commands) inspects recorded drawing
/// commands reachable from a committed layer tree. It does not evaluate clipping,
/// opacity, or GPU pixels; use it to distinguish painted content from retained
/// elements that have no commands in that tree.
pub mod rendering {
    pub use flui_rendering::testing::inspect::render_diagnostics;
    pub use flui_rendering::testing::{
        BoxQueryRun, DrawKind, FrameRun, LayoutRun, PaintRun, Probe, RenderTester, TreeNode,
        box_node, collect_commands, sliver_node,
    };
}
