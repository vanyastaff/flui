//! The framework's half of the profiler's wiring contract.
//!
//! `frame_timing_layer`'s unit tests emit the frame and phase spans
//! themselves, so they can only prove that the layer maps the right names.
//! This test drives a real tree through `HeadlessBinding::pump_frame`, which
//! runs the same `UpdateScheduler::drive_frame` the desktop, Android and web
//! runners run, and reads the profile back. If the scheduler stops opening
//! the `frame` span, or the pipeline renames a phase span, this is the test
//! that fails — the unit tests cannot.
#![cfg(feature = "profiling")]

use std::sync::Arc;
use std::time::Duration;

use flui_devtools::profiler::{FramePhase, FrameStats};
use flui_devtools::{FrameTimingLayer, Profiler};
use flui_objects::RenderSizedBox;
use flui_testing::{HeadlessBinding, MountOptions, MountOwners};
use flui_view::{RenderView, View};
use tracing::Dispatch;
use tracing_subscriber::layer::SubscriberExt;

/// The smallest tree that builds, lays out and paints: one render leaf.
#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn phases(stats: &FrameStats) -> Vec<FramePhase> {
    stats.phases.iter().map(|info| info.phase).collect()
}

#[test]
fn a_real_frame_reaches_the_profiler_with_its_phases() {
    // `tracing` caches callsite interest process-wide on first use; a sibling
    // test binary cannot poison this one, but the sentinel is cheap and it
    // is the documented way to capture under a thread-local subscriber.
    flui_testing::log_capture::disarm_interest_cache();

    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));

    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        let mounted =
            binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        assert!(mounted.painted, "the bootstrap frame must commit a paint");

        // The bootstrap runs the pipeline directly, outside `drive_frame`, so
        // it is not a frame: nothing may be attributed to a frame that never
        // opened. An empty history here is the honest reading.
        assert!(
            profiler.frame_history().is_empty(),
            "the bootstrap is not a frame; got {:?}",
            profiler.frame_history()
        );

        // An idle frame: build and layout run (their spans are unconditional),
        // there is nothing to paint.
        binding.pump_frame(Duration::from_millis(16));

        // Dirty the root and pump again: this frame paints.
        binding
            .pipeline_owner()
            .expect("mount_root leaves the binding tree-bound")
            .with_mut(|owner| owner.mark_needs_paint(mounted.render_root));
        binding.pump_frame(Duration::from_millis(16));
        assert!(
            binding.did_paint_last_frame(),
            "marking the root dirty must make the second pump paint"
        );
    });

    let history = profiler.frame_history();
    assert_eq!(
        history.len(),
        2,
        "one profiled frame per `drive_frame`, and only those; got {history:?}"
    );
    for (index, frame) in history.iter().enumerate() {
        let seen = phases(frame);
        assert!(
            seen.contains(&FramePhase::Build),
            "frame {index}: the pipeline's `build` span must land as Build; saw {seen:?}"
        );
        assert!(
            seen.contains(&FramePhase::Layout),
            "frame {index}: the pipeline's `layout` span must land as Layout; saw {seen:?}"
        );
        assert!(
            frame.total_time_ms() > 0.0,
            "frame {index}: a driven frame takes time; got {frame:?}"
        );
    }
    let painted = phases(&history[1]);
    assert!(
        painted.contains(&FramePhase::Paint),
        "the dirtied frame's `paint` span must land as Paint; saw {painted:?}"
    );
}
