//! Deterministic application tests, available with the `testing` feature.
//!
//! Enable `flui/testing` in a consumer's development dependency. The optional
//! test driver is absent from ordinary application dependency graphs.
//!
//! [`scenario::Tester`] mounts an application as `run_app` does, on a manual
//! clock, and drives it the way the driver and an agent do (ADR-0400):
//! [`scenario::find`] builds the `Find`s it looks with, over the window's
//! semantics tree, an action goes through the window's own input path, a
//! read runs no frame, and [`scenario::Tester::expect`] waits while the
//! application works for a [`scenario::matcher`] to hold. [`scenario::gesture`]
//! builds multi-pointer, keyed and timed input that `Tester::perform` sends, and [`frame`]
//! reads the presented frame in process, through types: a render object's
//! geometry or field, the draw commands under an element, the layer outline.
//!
//! ```
//! use flui::prelude::*;
//! use flui::testing::scenario::{Tester, find, matcher::exists};
//!
//! let mut t = Tester::mount(Center::new().child(Text::new("Hello")));
//! t.expect(&find::text("Hello"), exists());
//! ```
//!
//! The [`Tester`] and [`find`] at this module's root are the tester ADR-0400
//! replaces, deprecated until their users move.

pub use flui_testing::drop_ledger::{DropLedger, DropSentinel};
pub use flui_testing::find;
pub use flui_testing::frame;
pub use flui_testing::host::HostClock;
pub use flui_testing::perf::Cost;
pub use flui_testing::scenario;
pub use flui_testing::storage::{CommitBarrier, CopyHold, MemoryStorage, ReadHold, WriteHold};
#[expect(
    deprecated,
    reason = "ADR-0400: the old tester stays until its users move"
)]
pub use flui_testing::tester::{
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
