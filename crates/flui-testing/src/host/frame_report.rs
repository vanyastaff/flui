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
    /// A host whose last window closed runs no frame and reports none. The frame in which the
    /// window closes reports its pipeline work; its build and its scene went with the window.
    ///
    /// # Panics
    ///
    /// As [`pump`](Self::pump) does.
    pub(crate) fn pump_reported(&mut self, dt: Duration) -> FrameReport {
        let Some(runtime) = self.try_ui_runtime() else {
            let _outcome = self.pump(dt);
            return FrameReport::default();
        };
        let widgets = runtime.widgets();
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
        let pipeline = pipeline
            .with(PipelineOwner::counters)
            .since(pipeline_before);
        let (Some(runtime), Some(scene_before)) = (self.try_ui_runtime(), scene_before) else {
            return FrameReport {
                pipeline,
                ..FrameReport::default()
            };
        };
        let build = runtime
            .widgets()
            .with_build_owner_mut(|owner| owner.last_frame_build_report());
        let scene = self
            .scene_totals()
            .map_or_default(|after| SceneReport::between(&scene_before, &after));
        FrameReport {
            build,
            pipeline,
            scene,
        }
    }

    /// The window's retained-scene totals between frames, while the host has a window.
    fn scene_totals(&self) -> Option<flui_runtime::testing::SceneTotals> {
        let runtime = self.try_ui_runtime()?;
        runtime.presentation_scene_totals_for_test(runtime.presentation_id())
    }
}
