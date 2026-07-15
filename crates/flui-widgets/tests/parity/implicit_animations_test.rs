//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/lib/src/widgets/implicit_animations.dart`
//! (tag `3.44.0`) `AnimatedWidgetBaseState` —
//! `ImplicitlyAnimatedWidgetState`'s single `controller` drives every
//! `forEachTween` entry (`AnimatedContainer`'s `_widthTween`, `_heightTween`,
//! `_decorationTween`, ...) from the SAME `CurvedAnimation` value each build;
//! no per-property timing exists. No single upstream test in
//! `packages/flutter/test/widgets/animated_container_test.dart` isolates this
//! lockstep guarantee (it holds structurally, by construction, so Flutter
//! never needed a dedicated regression test for it) — this port makes the
//! architectural contract an explicit, falsifiable assertion.
//!
//! AnimatedOpacity-at-zero-still-hit-tests cites
//! `packages/flutter/lib/src/rendering/proxy_box.dart` `RenderOpacity` (line
//! 872) — it does not override `hitTestChildren`/`hitTestSelf` at all, so
//! opacity never gates hit-testing (unlike `RenderIgnorePointer`/
//! `RenderAbsorbPointer`, which exist specifically to do that).
//!
//! Widget → render-object mapping:
//! - `AnimatedContainer` → `Container` (rebuilt each tick) →
//!   `RenderConstrainedBox`/`RenderPadding` chain
//!   (`crates/flui-widgets/src/animated/animated_container.rs`).
//! - `AnimatedOpacity` → `RenderAnimatedOpacity` directly (no per-tick
//!   `Opacity` rebuild; `crates/flui-widgets/src/animated/animated_opacity.rs`,
//!   `crates/flui-objects/src/proxy/animated_opacity.rs`).
//!
//! Divergence: none for either case — FLUI's `AnimatedContainer` evaluates
//! every `OptTween` at the same `curved.value()` per build
//! (`crates/flui-widgets/src/animated/animated_container.rs:162-193`), and
//! `RenderAnimatedOpacity::hit_test`
//! (`crates/flui-objects/src/proxy/animated_opacity.rs`) hit-tests its child
//! unconditionally (gated only by `is_within_own_size`), never by opacity —
//! same as Flutter's `RenderAnimatedOpacityMixin`, which never overrides
//! `hitTest`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{self, lay_out, lay_out_animated, loose, tight};
use flui_animation::Vsync;
use flui_types::Color;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedContainer, AnimatedOpacity, ColoredBox, GestureDetector, VsyncScope};
use parking_lot::Mutex;

const FRAME: Duration = Duration::from_millis(20);
const RUN: Duration = Duration::from_millis(100);

// ----------------------------------------------------------------------------
// AnimatedContainer — multi-property lockstep
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct SizeProbe {
    vsync: Vsync,
    width: Arc<Mutex<f32>>,
    height: Arc<Mutex<f32>>,
}

struct SizeProbeState {
    vsync: Vsync,
    width: Arc<Mutex<f32>>,
    height: Arc<Mutex<f32>>,
}

impl StatefulView for SizeProbe {
    type State = SizeProbeState;

    fn create_state(&self) -> Self::State {
        SizeProbeState {
            vsync: self.vsync.clone(),
            width: Arc::clone(&self.width),
            height: Arc::clone(&self.height),
        }
    }
}

impl ViewState<SizeProbe> for SizeProbeState {
    fn build(&self, _view: &SizeProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let (width, height) = (*self.width.lock(), *self.height.lock());
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedContainer::new(flui_widgets::SizedBox::new(10.0, 10.0))
                .width(width)
                .height(height)
                .duration(RUN),
        )
    }
}

/// `AnimatedContainer` animating `width` AND `height` together must keep both
/// properties at exactly the same progress fraction on every frame — one
/// controller/curve drives every tween, not one timer per property.
///
/// Flutter parity: `AnimatedWidgetBaseState` (`implicit_animations.dart`, single shared curved animation driving all tweens)
/// — see the module-level citation above.
#[test]
fn animated_container_width_and_height_stay_in_lockstep_progress() {
    let vsync = Vsync::new();
    let width = Arc::new(Mutex::new(20.0));
    let height = Arc::new(Mutex::new(10.0));
    let probe = SizeProbe {
        vsync: vsync.clone(),
        width: Arc::clone(&width),
        height: Arc::clone(&height),
    };
    let mut laid = lay_out_animated(probe, loose(300.0), vsync);

    let dims = |laid: &common::LaidOut| -> (f32, f32) {
        let s = laid.size(laid.current_root());
        (s.width.get(), s.height.get())
    };
    assert_eq!(
        dims(&laid),
        (20.0, 10.0),
        "first frame holds the initial width/height with no motion"
    );

    // Retarget both properties to different new values and different
    // start→end spans (80px vs 40px) — if the two tweens were on independent
    // timers, their progress fractions would diverge; sharing one controller
    // guarantees they don't.
    *width.lock() = 100.0; // span 20 -> 100 (80px)
    *height.lock() = 50.0; // span 10 -> 50 (40px)
    laid.pump();
    laid.pump_for(FRAME); // detection frame: holds near the run-start values

    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(FRAME);
        samples.push(dims(&laid));
    }

    for &(w, h) in &samples {
        let width_progress = (w - 20.0) / (100.0 - 20.0);
        let height_progress = (h - 10.0) / (50.0 - 10.0);
        assert!(
            (width_progress - height_progress).abs() < 1e-3,
            "width and height must report the same progress fraction every frame \
             (one controller drives both): width_progress={width_progress}, \
             height_progress={height_progress}, sample=({w}, {h})"
        );
    }

    let (w_mid, _h_mid) = samples[1];
    assert!(
        w_mid > 21.0 && w_mid < 99.0,
        "an intermediate frame must show genuine mid-flight progress, got width={w_mid}"
    );
    let (w_last, h_last) = samples[4];
    assert!(
        (w_last - 100.0).abs() < 1.0 && (h_last - 50.0).abs() < 1.0,
        "the run must end at both new targets together: got ({w_last}, {h_last})"
    );
}

// ----------------------------------------------------------------------------
// AnimatedOpacity — opacity 0.0 still participates in hit-testing
// ----------------------------------------------------------------------------

/// `AnimatedOpacity` at `opacity: 0.0` is fully invisible but its child must
/// still receive pointer events — unlike `IgnorePointer`, opacity never gates
/// hit-testing in Flutter's render tree.
///
/// Flutter parity: `RenderOpacity` (`rendering/proxy_box.dart:872`) declares
/// no `hitTestChildren`/`hitTestSelf` override, so the inherited
/// `RenderProxyBox` behavior (hit-test the child unconditionally) is
/// unaffected by `opacity`.
#[test]
fn animated_opacity_at_zero_still_participates_in_hit_testing() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        AnimatedOpacity::new(
            0.0,
            GestureDetector::new()
                .on_tap(move || {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(ColoredBox::new(Color::rgb(200, 30, 30))),
        ),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a fully transparent (opacity 0.0) AnimatedOpacity must still deliver taps to its child"
    );
}
