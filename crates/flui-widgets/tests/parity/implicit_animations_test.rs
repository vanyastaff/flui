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
//!
//! ## AnimatedOpacity — the remaining `implicit_animations_test.dart` cases
//!
//! Ported: 2 more (4 total counted for this widget, see below).
//! - `'AnimatedOpacity transition test'`
//! - `'AnimatedOpacity does not crash at zero area'`
//!
//! Out of scope: 1.
//! - `'AnimatedOpacity onEnd callback test'` — **`AnimatedOpacity` has no
//!   `on_end` callback field at all**, unlike its sibling `AnimatedSize`
//!   (`crates/flui-widgets/src/animated/animated_size.rs`'s `on_end`), so
//!   there is nothing to invoke — a real, widget-specific capability gap,
//!   not a silent skip.
//!
//! Citation-only: 1.
//! - `'Ensure CurvedAnimations are disposed on widget change'` — this suite
//!   already carries the equivalent assertion at the unit level:
//!   `unrelated_rebuild_does_not_swap_the_proxy_parent`,
//!   `opacity_retarget_swaps_the_proxy_parent`, and
//!   `curve_only_change_swaps_the_proxy_parent`
//!   (`crates/flui-widgets/src/animated/animated_opacity.rs`'s
//!   `#[cfg(test)] mod tests`) — FLUI's `AnimatedOpacity` has no persistent
//!   `CurvedAnimation` to directly query `isDisposed` on (it composes a
//!   fresh `tween.animate(curved)` per retarget behind a `ProxyAnimation`,
//!   see that module's docs), so "the old composition is discarded and the
//!   render object observes only the new one" is exactly what those 3 tests
//!   already pin via `Arc::ptr_eq` on the proxy's parent.
//!
//! `'AnimatedOpacity transition test'` uses `Curves::Linear` explicitly
//! rather than the family's zero-arg default: the oracle's `TestAnimatedWidget`
//! harness relies on `ImplicitlyAnimatedWidget`'s OWN default curve
//! (`Curves.linear`, `implicit_animations.dart:288`), but FLUI's
//! `default_curve()` (`crates/flui-widgets/src/animated/implicitly_animated.rs`)
//! is `Curves::EaseInOut` for every widget in this family — a family-wide
//! default-curve divergence from the oracle, undocumented before this port.
//! Changing the shared default is a behavioral change with a much wider
//! blast radius than this test port (every implicit-animation widget with no
//! explicit `.curve(...)` override), so it is out of scope here; this test
//! instead pins its own curve explicitly to isolate the interpolation-value
//! assertions from that divergence.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{self, lay_out, lay_out_animated, loose, tight};
use flui_animation::{Curves, Vsync};
use flui_types::Color;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{
    AnimatedContainer, AnimatedOpacity, ColoredBox, GestureDetector, SizedBox, VsyncScope,
};
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

// ----------------------------------------------------------------------------
// AnimatedOpacity — transition test
// ----------------------------------------------------------------------------

/// The ancestor hosting `AnimatedOpacity` — mirrors the oracle's
/// `RebuildCountingState<TestAnimatedWidget>` (`implicit_animations_test.dart`):
/// `builds` counts every time THIS build runs, so the transition test can
/// assert the ancestor is rebuilt on the retarget (a real `setState`) and
/// NOT on every subsequent tick (`AnimatedOpacity` updates its persistent
/// render object directly, per its module docs — it never rebuilds through
/// `AnimatedBuilder` the way `AnimatedContainer`/`AnimatedAlign`/
/// `AnimatedPadding` do).
#[derive(Clone, StatefulView)]
struct OpacityProbe {
    vsync: Vsync,
    opacity: Arc<Mutex<f32>>,
    builds: Arc<AtomicUsize>,
}

struct OpacityProbeState {
    vsync: Vsync,
    opacity: Arc<Mutex<f32>>,
    builds: Arc<AtomicUsize>,
}

impl StatefulView for OpacityProbe {
    type State = OpacityProbeState;

    fn create_state(&self) -> Self::State {
        OpacityProbeState {
            vsync: self.vsync.clone(),
            opacity: Arc::clone(&self.opacity),
            builds: Arc::clone(&self.builds),
        }
    }
}

impl ViewState<OpacityProbe> for OpacityProbeState {
    fn build(&self, _view: &OpacityProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.fetch_add(1, Ordering::SeqCst);
        let opacity = *self.opacity.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedOpacity::new(opacity, SizedBox::shrink())
                .duration(Duration::from_secs(1))
                .curve(Curves::Linear),
        )
    }
}

/// `AnimatedOpacity` genuinely interpolates from the current opacity to a
/// retargeted one: `0.0` right after the retarget (t=0, begin unchanged),
/// then exactly `0.5`/`0.75`/`1.0` at 50%/75%/100% of a 1-second linear run —
/// and the ANCESTOR hosting it (`OpacityProbe`, standing in for the oracle's
/// `TestAnimatedWidget`) is rebuilt exactly once for the retarget itself and
/// NOT again on any of the subsequent animation ticks.
///
/// Flutter parity: `'AnimatedOpacity transition test'`
/// (`implicit_animations_test.dart`, tag `3.44.0`) — same 1-second duration,
/// same 0.0 → 1.0 retarget, same sample points (500 ms, 750 ms, 1000 ms
/// cumulative), and the oracle's own `state.builds == 2` assertions (stable
/// across every tick after the retargeting rebuild) via the `builds` counter
/// below. `Curves::Linear` is pinned explicitly — see the module docs' note
/// on the family's default-curve divergence from the oracle.
#[test]
fn animated_opacity_transition_test() {
    let vsync = Vsync::new();
    let opacity = Arc::new(Mutex::new(0.0));
    let builds = Arc::new(AtomicUsize::new(0));
    let probe = OpacityProbe {
        vsync: vsync.clone(),
        opacity: Arc::clone(&opacity),
        builds: Arc::clone(&builds),
    };
    let mut laid = lay_out_animated(probe, tight(100.0, 100.0), vsync);
    let id = laid.find_by_render_type("RenderAnimatedOpacity");

    assert_eq!(
        laid.opacity(id),
        0.0,
        "first build sits at the given opacity"
    );
    assert_eq!(builds.load(Ordering::SeqCst), 1, "the initial mount build");

    *opacity.lock() = 1.0;
    laid.pump();
    assert_eq!(
        laid.opacity(id),
        0.0,
        "immediately after retargeting, the run has not advanced (t=0, begin unchanged)"
    );
    assert_eq!(
        builds.load(Ordering::SeqCst),
        2,
        "the retarget is a real ancestor rebuild (oracle: state.builds == 2)"
    );

    laid.pump_for(Duration::ZERO); // detection frame: anchors the fresh run
    laid.pump_for(Duration::from_millis(500));
    assert_eq!(laid.opacity(id), 0.5, "50% through a linear run");
    assert_eq!(
        builds.load(Ordering::SeqCst),
        2,
        "a tick alone must NOT rebuild the ancestor — AnimatedOpacity updates \
         its persistent render object directly (oracle: state.builds stays 2)"
    );

    laid.pump_for(Duration::from_millis(250)); // cumulative 750 ms
    assert_eq!(laid.opacity(id), 0.75, "75% through a linear run");
    assert_eq!(
        builds.load(Ordering::SeqCst),
        2,
        "still no ancestor rebuild"
    );

    laid.pump_for(Duration::from_millis(250)); // cumulative 1000 ms
    assert_eq!(
        laid.opacity(id),
        1.0,
        "the run must end exactly at the new target"
    );
    assert_eq!(
        builds.load(Ordering::SeqCst),
        2,
        "no ancestor rebuild even once the run completes"
    );
}

// ----------------------------------------------------------------------------
// AnimatedOpacity — zero-area guard
// ----------------------------------------------------------------------------

/// `AnimatedOpacity` on a zero-area surface lays out to zero without
/// panicking.
///
/// Flutter parity: `'AnimatedOpacity does not crash at zero area'`
/// (`implicit_animations_test.dart`, tag `3.44.0`).
#[test]
fn animated_opacity_does_not_crash_at_zero_area() {
    let mut laid = lay_out(
        SizedBox::shrink().child(
            AnimatedOpacity::new(0.5, SizedBox::shrink()).duration(Duration::from_millis(300)),
        ),
        tight(0.0, 0.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "AnimatedOpacity on a zero-area surface must measure 0×0 with no panic"
    );

    // The oracle reaches its assertion via `pumpAndSettle`; a never-pumped
    // mount would not catch a panic that only surfaces once a frame runs.
    laid.pump_for(Duration::from_millis(300));
    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "must still measure 0×0 with no panic after a frame runs"
    );
}
