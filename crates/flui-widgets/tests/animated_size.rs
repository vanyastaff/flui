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

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{lay_out_animated, loose};
use flui_animation::Vsync;
use flui_painting::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{EventCx, IntoView, ViewState};
use flui_widgets::{AnimatedSize, SizedBox, VsyncScope};
use parking_lot::Mutex;

/// A 100 ms run pumped in 20 ms frames spans the run in five steps.
const FRAME: Duration = Duration::from_millis(20);
const RUN: Duration = Duration::from_millis(100);

type EndCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

#[derive(Clone, StatefulView)]
struct SizeProbe {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
    alignment: Arc<Mutex<Alignment>>,
    on_end: Option<EndCallback>,
}

struct SizeProbeState {
    vsync: Vsync,
    side: Arc<Mutex<f64>>,
    alignment: Arc<Mutex<Alignment>>,
    on_end: Option<EndCallback>,
}

impl StatefulView for SizeProbe {
    type State = SizeProbeState;

    fn create_state(&self) -> Self::State {
        SizeProbeState {
            vsync: self.vsync.clone(),
            side: Arc::clone(&self.side),
            alignment: Arc::clone(&self.alignment),
            on_end: self.on_end.clone(),
        }
    }
}

impl ViewState<SizeProbe> for SizeProbeState {
    fn build(&self, _view: &SizeProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let side = *self.side.lock();
        let alignment = *self.alignment.lock();
        let mut animated = AnimatedSize::new(RUN)
            .alignment(alignment)
            .child(SizedBox::new(side, side));
        if let Some(on_end) = self.on_end.clone() {
            animated = animated.on_end(move |cx| on_end(cx));
        }
        VsyncScope::new(self.vsync.clone(), animated)
    }
}

fn width(laid: &crate::common::LaidOut) -> f64 {
    laid.size(laid.current_root()).width
}

pub(crate) fn animated_size_interpolates_to_a_new_child_size_over_frames() {
    let vsync = Vsync::new();
    let side = Arc::new(Mutex::new(20.0));
    let probe = SizeProbe {
        vsync: vsync.clone(),
        side: Arc::clone(&side),
        alignment: Arc::new(Mutex::new(Alignment::CENTER)),
        on_end: None,
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
