//! # `flui_testing`
//!
//! The workspace's test support. Its frames are the product frame
//! transaction: [`HeadlessHost`] hosts a UI runtime over a headless window and
//! sink, and each [`pump`](HeadlessHost::pump) advances the host's virtual
//! clock and runs `UiRuntime::pump` at the new instant (ADR-0083 §4). Every
//! deadline-driven gesture (long-press, the press-delay of double-tap) and
//! every animation therefore runs on one virtual timeline, so tests are
//! deterministic with **no wall-clock `thread::sleep`**.
//!
//! A host is an ordinary value, not a process-global binding: many can exist
//! at once, so test suites run in parallel without contending on shared
//! state. Two presentations that need independent clocks are two hosts.
//!
//! - [`widgets::lay_out`] mounts a widget tree in a host and adds geometry,
//!   layer and semantics probes; [`Tester`] is the application test API.
//! - [`replay`] scripts pointer input as data on virtual time;
//!   [`HeadlessHost::run_script`] and [`LaidOut::replay`](widgets::LaidOut::replay)
//!   deliver it to the UI runtime's input path.
//!
//! ### What a frame cost
//!
//! [`LaidOut::pump_report`](crate::widgets::LaidOut::pump_report) drives one
//! frame of the UI runtime and returns its [`FrameReport`]: the build owner's
//! report (distinct elements rebuilt, builds run, the per-reason split), the
//! pipeline's phase counters differenced across the frame and what its scene
//! patch cost. `flui-widgets`' `perf` test target drives a fixed app through idle,
//! scroll, text-change and full-reassemble frames, asserts budgets on those
//! reports, and — when `FLUI_PERF_OUT` names a directory — writes each
//! scenario's counts there for `cargo xtask perf` to compare with the
//! checked-in baseline.
//!
//! ## Example
//!
//! A long press is a contact held past a deadline on the host's virtual
//! clock: the same script held for less time is not one.
//!
//! ```
//! use std::{cell::Cell, rc::Rc, time::Duration};
//!
//! use flui_foundation::geometry::Offset;
//! use flui_painting::styling::Color;
//! use flui_testing::replay::{PointerScript, ScriptError, TouchScript};
//! use flui_testing::{HeadlessHost, HeadlessWindow};
//! use flui_widgets::{ColoredBox, GestureDetector};
//!
//! let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
//! let fired = Rc::new(Cell::new(false));
//! let in_callback = Rc::clone(&fired);
//! host.attach(
//!     &GestureDetector::new()
//!         .on_long_press(move |_| in_callback.set(true))
//!         .child(ColoredBox::new(Color::RED)),
//! )
//! .expect("the host has no root yet");
//! let _ = host.pump(Duration::from_millis(16));
//!
//! let press = |hold: Duration| -> Result<PointerScript, ScriptError> {
//!     TouchScript::new("press")
//!         .down(1, Offset::new(100.0, 100.0))
//!         .wait(hold)
//!         .frame()
//!         .up(1)
//!         .build()
//! };
//!
//! // 300 ms of virtual time: the long-press deadline has not elapsed.
//! host.run_script(&press(Duration::from_millis(300))?);
//! assert!(!fired.get());
//!
//! // Held for 800 ms, the frame inside the hold fires the deadline.
//! host.run_script(&press(Duration::from_millis(800))?);
//! assert!(fired.get());
//! # Ok::<(), ScriptError>(())
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Every public item is documented; keep it that way.
#![deny(missing_docs)]

pub mod a11y;
pub mod agent_service;
pub mod backend_conformance;
pub mod clipboard;
pub mod control_cost;
pub mod drop_ledger;
pub mod find;
pub mod gesture_alternatives;
pub mod heap;
pub mod host;
pub mod motion;
// The log capture lives in its subscriber's crate (`flui_log::testing`); this
// path keeps its name for the kit's consumers.
pub use flui_log::testing as log_capture;
pub mod ownership;
pub mod portal_exports;
pub mod primary_selection;
pub mod recording;
pub mod replay;
pub mod storage;
pub mod tester;
pub mod text_store_host;
pub mod text_store_kit;
pub mod widgets;
pub mod window_commands;
pub mod window_conformance;
pub mod window_handler;

pub use a11y::{
    A11yNode, A11yQuery, A11yQueryError, A11yTree, Action, ActionData, ActionRequest,
    InvokeActionError, NodeId, TreeId,
};
pub use agent_service::{ConversationRefused, HeadlessAgentService, HeadlessConversation};
pub use backend_conformance::{BackendUnderTest, assert_backend_conforms};
pub use clipboard::ScriptedClipboard;
pub use drop_ledger::{DropLedger, DropSentinel};
pub use flui_platform::testing::{
    MockWindow, ScriptedHttp, ScriptedResponse, WindowCall, mock_window,
};
pub use flui_protocol::{
    geometry::LogicalPoint, geometry::LogicalRect, inspect::InspectField, inspect::InspectNode,
    inspect::InspectReading, inspect::InspectValue, locator::Locator, locator::Match,
    locator::Pick, locator::StateFilter, locator::TextMatch, tree::Node, tree::Tree, wire::Role,
};
pub use flui_runtime::presentation::{PointerResampling, PointerResamplingError};
pub use gesture_alternatives::{
    AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives,
};
pub use host::{
    HeadlessDevAgent, HeadlessHost, HeadlessOsDrags, HeadlessSink, HeadlessWindow,
    HeadlessWindowId, HostClock, NotSettled, SESSION_END_BUDGET, ScriptedTextSizing, StartedOsDrag,
    UnshownFrame, WindowSystemMismatch,
};
pub use log_capture::{CapturedLog, CapturedRecord, capture, disarm_interest_cache};
pub use ownership::assert_ownership_table;
pub use portal_exports::HeadlessPortalExports;
pub use primary_selection::ScriptedPrimarySelection;
pub use replay::{
    MouseScript, PenScript, PinchPhase, PointerPhase, PointerScript, ScriptError, ScriptStep,
    ScriptedPointer, TouchScript, TrackpadScript, VirtualScript,
};
pub use tester::{Expectation, TestError, TestWindow, Tester};
pub use text_store_host::{RecordingTextStoreHost, StoreHostCall};

/// What one frame did, phase by phase: what [`LaidOut::pump_report`](crate::widgets::LaidOut::pump_report)
/// returns for a frame of the UI runtime.
///
/// Counts, not timings: every figure is a deterministic function of the tree
/// and the change applied, which is what lets `cargo xtask perf` compare them
/// exactly against a checked-in baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameReport {
    /// The build owner's report for the frame (distinct elements rebuilt,
    /// builds run, per-reason split).
    pub build: flui_view::FrameBuildReport,
    /// The pipeline's work, differenced across the frame.
    pub pipeline: flui_rendering::pipeline::PipelineCounters,
    /// The retained scene's work: what the frame's patch changed and what
    /// applying it cost (ADR-0304 §4, §8).
    pub scene: SceneReport,
}

impl FrameReport {
    /// The report as a stable `(name, value)` list, in a fixed order. The
    /// names are the perf baseline's keys, so renaming one is a baseline
    /// change.
    #[must_use]
    pub fn counters(&self) -> [(&'static str, u64); 15] {
        let p = &self.pipeline;
        [
            ("elements_rebuilt", self.build.elements_built as u64),
            ("builds_run", self.build.builds_run as u64),
            ("layout_passes", p.layout_passes),
            ("layout_roots", p.layout_roots),
            ("nodes_laid_out", p.nodes_laid_out),
            ("nodes_painted", p.nodes_painted),
            ("layers_produced", p.layers_produced),
            ("layers_reused", p.layers_reused),
            ("semantics_nodes_updated", p.semantics_nodes_updated),
            ("frames_produced", p.frames_produced),
            ("scene_ops", p.scene_ops),
            ("damage_rects", self.scene.damage_rects),
            ("damage_full", self.scene.damage_full),
            ("scene_nodes_visited", self.scene.scene_nodes_visited),
            ("scene_depth_updates", self.scene.scene_depth_updates),
        ]
    }
}

impl std::ops::AddAssign<&FrameReport> for FrameReport {
    /// Sums two frames: every counter adds, and the per-reason split merges
    /// by reason. `elements_rebuilt` of a sum is the sum of the per-frame
    /// distinct counts, not a distinct count across frames.
    fn add_assign(&mut self, rhs: &FrameReport) {
        self.build.elements_built += rhs.build.elements_built;
        self.build.builds_run += rhs.build.builds_run;
        for &(reason, count) in &rhs.build.by_reason {
            match self
                .build
                .by_reason
                .iter_mut()
                .find(|(existing, _)| *existing == reason)
            {
                Some((_, total)) => *total += count,
                None => self.build.by_reason.push((reason, count)),
            }
        }
        self.pipeline += rhs.pipeline;
        self.scene += rhs.scene;
    }
}

/// What one frame's scene patch did, as the perf baseline counts it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SceneReport {
    /// Rectangles of the patch's damage; `0` for a frame that applied none
    /// or damaged nothing.
    pub damage_rects: u64,
    /// Patches whose damage was the whole surface.
    pub damage_full: u64,
    /// Scene nodes the apply copied or created.
    pub scene_nodes_visited: u64,
    /// Node depths the apply recomputed.
    pub scene_depth_updates: u64,
}

impl SceneReport {
    /// The work a frame did between two readings of a runtime window's
    /// scene: the damage its new scene version carries (a patch's, or a
    /// renewal's full damage, since a renewed scene is presented whole), and
    /// the nodes and depths the frame touched.
    pub(crate) fn between(
        before: &flui_runtime::testing::SceneTotals,
        after: &flui_runtime::testing::SceneTotals,
    ) -> Self {
        let (damage_rects, damage_full) = if after.version == before.version {
            (0, 0)
        } else {
            match &after.patch_damage {
                flui_rendering::layer::DamageRegion::Unchanged => (0, 0),
                flui_rendering::layer::DamageRegion::Region(region) => {
                    (region.rects().len() as u64, 0)
                }
                flui_rendering::layer::DamageRegion::Full(_) => (0, 1),
            }
        };
        Self {
            damage_rects,
            damage_full,
            scene_nodes_visited: after.nodes_visited - before.nodes_visited,
            scene_depth_updates: after.depth_updates - before.depth_updates,
        }
    }
}

impl std::ops::AddAssign for SceneReport {
    fn add_assign(&mut self, rhs: Self) {
        self.damage_rects += rhs.damage_rects;
        self.damage_full += rhs.damage_full;
        self.scene_nodes_visited += rhs.scene_nodes_visited;
        self.scene_depth_updates += rhs.scene_depth_updates;
    }
}

/// Takes the author errors the last frame's publication found: a semantics
/// attribute its node's role does not support, left unpublished, or (in debug
/// builds) siblings sharing an identifier. A test should fail on them, where an
/// application only logs them; `HeadlessHost::pump` takes them for every open
/// window after each frame and fails a frame that failed nothing else. It
/// reads what the pipeline recorded, not the report queue, so a reporting
/// subscriber that drains the queue, replaces the semantics owner or disables
/// semantics hides nothing from it.
pub(crate) fn take_semantics_findings(
    pipeline: &flui_rendering::pipeline::PipelineCell,
) -> Vec<flui_rendering::semantics::SemanticsDiagnostic> {
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::semantics::SemanticsDiagnostic;

    let mut findings = pipeline.with_mut(PipelineOwner::take_unchecked_semantics_diagnostics);
    // Withheld text runs report an exhausted identity space, not an author
    // error; a test that exhausts it reads the report queue.
    findings.retain(|finding| !matches!(finding, SemanticsDiagnostic::TextRunsWithheld(_)));
    findings
}
