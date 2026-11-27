//! A frame and what it did: [`HeadlessHost::pump_reported`], the one count of a frame's work that
//! a perf scenario's baseline holds, whichever harness ran the frame.

use std::time::Duration;

use flui_rendering::pipeline::PipelineOwner;

use super::HeadlessHost;
use crate::{FrameReport, SceneReport};

impl HeadlessHost {
    /// Run one frame `dt` after the last, as [`pump`](Self::pump) does, and report what it did:
    /// the build owner's report of the frame, the pipeline's counters across it and what its
    /// scene patch cost. Counts, not timings, so a perf scenario compares them exactly against a
    /// checked-in baseline.
    ///
    /// # Panics
    ///
    /// As [`pump`](Self::pump) does, and once the host's last window closed.
    pub(crate) fn pump_reported(&mut self, dt: Duration) -> FrameReport {
        let widgets = self.ui_runtime().widgets();
        let pipeline = widgets
            .pipeline_owner()
            .expect("BUG: a ui_runtime's presentation installs its pipeline");
        let pipeline_before = pipeline.with(PipelineOwner::counters);
        let scene_before = self.scene_totals();
        // Cleared before the frame, the build owner's report holds nothing an earlier frame
        // built: empty when no build pass ran. When a focus move rebuilds within the frame, a
        // second pass that builds replaces the first pass's report
        // (`WidgetsBinding::rebuild_within_frame`).
        widgets.with_build_owner_mut(
            flui_view::__runtime::BuildOwner::clear_frame_build_report_for_test,
        );
        let _outcome = self.pump(dt);
        let pipeline_after = pipeline.with(PipelineOwner::counters);
        let scene_after = self.scene_totals();
        let build = self
            .ui_runtime()
            .widgets()
            .with_build_owner_mut(|owner| owner.last_frame_build_report());
        FrameReport {
            build,
            pipeline: pipeline_after.since(pipeline_before),
            scene: SceneReport::between(&scene_before, &scene_after),
        }
    }

    /// The window's retained-scene totals between frames.
    fn scene_totals(&self) -> flui_runtime::testing::SceneTotals {
        let runtime = self.ui_runtime();
        runtime
            .presentation_scene_totals_for_test(runtime.presentation_id())
            .expect("BUG: between frames the window's scene is in its slot")
    }
}
