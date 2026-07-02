//! [`AnimatedSize`] driven deterministically through the headless binding: a
//! child whose natural size changes animates the container toward it over
//! frames instead of snapping — mirroring `implicit_animations.rs`'s pattern
//! for the sibling implicitly-animated widgets.
//!
//! The second test is the regression guard for this widget's one deliberate
//! structural divergence from every sibling (`AnimatedOpacity`, `AnimatedAlign`,
//! …): `AnimatedSizeRenderView::update_render_object` must reach the
//! persistent `RenderAnimatedSize` through targeted setters, never by
//! replacing the render object (the `Align`/`RenderAlign` convention) — doing
//! the latter would silently reset the in-flight retarget state on every
//! unrelated rebuild.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{lay_out_animated, loose};
use flui_animation::Vsync;
use flui_types::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedSize, SizedBox, VsyncScope};
use parking_lot::Mutex;

/// A 100 ms run pumped in 20 ms frames spans the run in five steps.
const FRAME: Duration = Duration::from_millis(20);
const RUN: Duration = Duration::from_millis(100);

#[derive(Clone, StatefulView)]
struct SizeProbe {
    vsync: Vsync,
    side: Arc<Mutex<f32>>,
    alignment: Arc<Mutex<Alignment>>,
}

struct SizeProbeState {
    vsync: Vsync,
    side: Arc<Mutex<f32>>,
    alignment: Arc<Mutex<Alignment>>,
}

impl StatefulView for SizeProbe {
    type State = SizeProbeState;

    fn create_state(&self) -> Self::State {
        SizeProbeState {
            vsync: self.vsync.clone(),
            side: Arc::clone(&self.side),
            alignment: Arc::clone(&self.alignment),
        }
    }
}

impl ViewState<SizeProbe> for SizeProbeState {
    fn build(&self, _view: &SizeProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.side.lock();
        let alignment = *self.alignment.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedSize::new(RUN)
                .alignment(alignment)
                .child(SizedBox::new(side, side)),
        )
    }
}

fn width(laid: &common::LaidOut) -> f32 {
    laid.size(laid.current_root()).width.get()
}

#[test]
fn animated_size_first_frame_snaps_to_child_size_with_no_motion() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let probe = SizeProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
        alignment: Arc::new(Mutex::new(Alignment::CENTER)),
    };
    let laid = lay_out_animated(probe, loose(200.0), vsync);

    // No configuration change yet: the widget sits AT the child's size, no
    // animation.
    assert!(
        (width(&laid) - 20.0).abs() < 1e-4,
        "first frame shows the child's raw size, got {}",
        width(&laid),
    );
}

#[test]
fn animated_size_interpolates_to_a_new_child_size_over_frames() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let probe = SizeProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
        alignment: Arc::new(Mutex::new(Alignment::CENTER)),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);

    assert!((width(&laid) - 20.0).abs() < 1e-3, "starts at 20px");

    // Swap in a bigger child (same `SizedBox` type at the same tree
    // position — reconciled in place, not remounted): the AnimatedSize
    // reconciles, its RenderAnimatedSize sees the child's new natural size
    // next layout, and starts a run from 20 toward 100.
    *side.lock() = 100.0;
    laid.pump();

    // The detection frame (first pump_for after the retarget) still holds
    // the run-start value: the controller's first tick anchors its epoch.
    laid.pump_for(FRAME);
    assert!(
        width(&laid) < 21.0,
        "first frame after retarget holds near the start, got {}",
        width(&laid),
    );

    // Five 20 ms frames over the 100 ms run climb monotonically toward 100.
    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(FRAME);
        samples.push(width(&laid));
    }
    for pair in samples.windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-3,
            "width must not regress across frames: {samples:?}",
        );
    }
    let intermediate = samples[1];
    assert!(
        intermediate > 21.0 && intermediate < 99.0,
        "an intermediate frame shows a partial width, got {intermediate}",
    );
    assert!(
        (samples[4] - 100.0).abs() < 1.0,
        "the run ends at the new 100px width, got {}",
        samples[4],
    );
}

#[test]
fn animated_size_unrelated_rebuild_does_not_reset_in_flight_animation() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let alignment = Arc::new(Mutex::new(Alignment::CENTER));
    let probe = SizeProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
        alignment: Arc::clone(&alignment),
    };
    let mut laid = lay_out_animated(probe, loose(200.0), vsync);

    *side.lock() = 100.0;
    laid.pump();
    laid.pump_for(FRAME); // detection frame
    laid.pump_for(FRAME);
    let before_unrelated_rebuild = width(&laid);
    assert!(
        before_unrelated_rebuild > 20.5 && before_unrelated_rebuild < 99.5,
        "must be genuinely mid-flight before the unrelated rebuild, got {before_unrelated_rebuild}",
    );

    // An UNRELATED rebuild: only `alignment` changes, `side` does not. If
    // `update_render_object` replaced the whole render object (the `Align`
    // convention this widget's docs explicitly warn against), this would
    // reset `size_tween`/`state`/the controller subscription and the
    // reported size would snap straight to the child's raw current size
    // (100) instead of continuing from `before_unrelated_rebuild`.
    *alignment.lock() = Alignment::BOTTOM_RIGHT;
    laid.pump();
    let after_unrelated_rebuild = width(&laid);
    assert!(
        (after_unrelated_rebuild - before_unrelated_rebuild).abs() < 1e-3,
        "an unrelated (alignment-only) rebuild must not perturb the \
         in-flight animated size — before {before_unrelated_rebuild}, \
         after {after_unrelated_rebuild}",
    );

    // The alignment change DID reach the persistent render object: while
    // still mid-flight (reported size < the child's full 100px), BOTTOM_RIGHT
    // (factor 1.0) must offset the child by exactly `size - child_size`,
    // discriminating it from the stale CENTER (factor 0.5) offset a
    // whole-object-replace bug would also have reset back to.
    let child = laid.only_child(laid.current_root());
    let child_offset = laid.offset(child);
    let expected_dx = after_unrelated_rebuild - 100.0;
    assert!(
        (child_offset.dx.get() - expected_dx).abs() < 1.0,
        "BOTTOM_RIGHT must reach the persistent render object via the \
         targeted setter — child offset {child_offset:?}, expected dx≈{expected_dx}",
    );

    // The animation must still be running: further frames keep climbing
    // toward the target, proving the controller subscription survived the
    // unrelated rebuild (an object-replace would have torn it down).
    for _ in 0..5 {
        laid.pump_for(FRAME);
    }
    assert!(
        (width(&laid) - 100.0).abs() < 1.0,
        "the animation must still converge to the target after the \
         unrelated rebuild, got {}",
        width(&laid),
    );
}
