//! Deterministic application tests, available with the `testing` feature.
//!
//! Enable `flui/testing` in a consumer's development dependency. The optional
//! test driver is absent from ordinary application dependency graphs.
//!
//! [`Tester`] mounts an application as `run_app` does, on a manual clock, and
//! drives it the way the driver and an agent do: [`find`] builds the
//! [`Locator`]s it looks with, over the window's semantics tree, an action
//! goes through the window's own input path, and [`Tester::expect`] waits
//! while the application works.
//!
//! ```
//! use flui::prelude::*;
//! use flui::testing::{Tester, find};
//!
//! let mut t = Tester::mount(Center::new().child(Text::new("Hello")));
//! t.expect(find::text("Hello")).to_exist();
//! ```

pub use flui_testing::find;
pub use flui_testing::{
    Expectation, InspectField, InspectNode, InspectReading, InspectValue, Locator, LogicalPoint,
    LogicalRect, Match, Node, Pick, Role, StateFilter, TestError, TestWindow, Tester, TextMatch,
    Tree,
};

/// Render-object tests that drive layout and queries without a widget tree,
/// for the author of a render object of their own.
/// [`collect_commands`](rendering::collect_commands) inspects recorded drawing
/// commands reachable from a committed layer tree. It does not evaluate clipping,
/// opacity, or GPU pixels; use it to distinguish painted content from retained
/// elements that have no commands in that tree.
pub mod rendering {
    pub use flui_rendering::testing::{
        BoxQueryRun, DrawCommandSummary, DrawKind, FrameRun, LayoutRun, PaintRun, Probe,
        RenderTester, TreeNode, box_node, collect_commands, sliver_node,
    };
}

/// The harnesses `flui::testing` exported before [`Tester`]: the substrate
/// driver, the widget layout harness, gesture replay, the accessibility
/// reader and the teardown and storage doubles. The facade's own consumer
/// fixtures are their last users ([gaps](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#the-facades-old-test-harnesses)).
///
/// The compiler reports a deprecated module only for the items the module
/// defines, and this one only re-exports, so a path through it raises no
/// warning: this marker and the gaps entry are the notice, and the module
/// goes before a release.
#[deprecated(note = "use flui::testing::Tester and flui::testing::find")]
pub mod harness {
    pub use flui_rendering::testing::inspect::render_snapshot;
    pub use flui_testing::{
        BuildCapabilities, HeadlessBinding, MountOptions, MountOwners, Mounted,
    };
    pub use flui_testing::{DropLedger, DropSentinel};
    pub use flui_testing::{a11y, replay, storage, widgets};
}
