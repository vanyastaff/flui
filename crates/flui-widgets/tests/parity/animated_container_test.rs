//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/animated_container_test.dart`
//! (tag `3.44.0`).
//!
//! Widget → render-object mapping:
//! - `AnimatedContainer` → `Container` (rebuilt via `AnimatedBuilder` each
//!   tick) → `RenderConstrainedBox`/`RenderPadding` chain
//!   (`crates/flui-widgets/src/animated/animated_container.rs`).
//!
//! Ported: 2 of 9 oracle cases (a 3rd — the "genuine change only" gating —
//! is ported as an inline unit test in `animated_container.rs` itself, not
//! here; see below).
//! - `'AnimatedContainer does not crash at zero area'`
//! - `'Animation rerun'` — partially: this suite's existing
//!   `implicit_animations_test.rs::animated_container_width_and_height_stay_in_lockstep_progress`
//!   already covers "retarget both width and height together, assert
//!   mid-flight bounds and exact settle" (the core of `Animation rerun`), so
//!   that portion is citation-only. This file adds the one scenario that test
//!   does NOT cover: retargeting a SINGLE property (height) while a SIBLING
//!   property (width) is held at an unchanged target, proving `OptTween`'s
//!   Some→Some-unchanged branch does not perturb the untouched property.
//!
//! Citation-only: 1 of 9.
//! - `'Animation rerun'` (mid-flight-bounds + exact-settle portion) — see
//!   `implicit_animations_test.rs::animated_container_width_and_height_stay_in_lockstep_progress`.
//!
//! Out of scope: 6 of 9.
//! - `'AnimatedContainer.debugFillProperties'` — exercises `decoration`,
//!   `foregroundDecoration`, and `transform`, none of which
//!   `AnimatedContainer` animates yet (see its module doc); also depends on
//!   Dart's `hasOneLineDescription` Debug-format parity, which this harness
//!   has no equivalent for.
//! - `'AnimatedContainer control test'` — animates `decoration`;
//!   `AnimatedContainer`'s module doc states decoration is not yet animated
//!   (passes straight through when set, no tween).
//! - `'AnimatedContainer padding visual-to-directional animation'` and
//!   `'AnimatedContainer alignment visual-to-directional animation'` —
//!   `AnimatedContainer::padding`/`alignment` are plain `EdgeInsets`/`Alignment`
//!   (`Container`-family-wide, not a gap unique to this widget), never
//!   `EdgeInsetsGeometry`/`AlignmentGeometry`; there is no
//!   `Directionality`-resolved path on this widget to exercise.
//! - `'AnimatedContainer sets transformAlignment'` — `transform`/
//!   `transformAlignment` are not implemented on `Container` at all.
//! - `'AnimatedContainer sets clipBehavior'` — `Container` has no
//!   `clip_behavior` field.
//!
//! The "genuine change only" case (adapted from `'AnimatedContainer
//! overanimate test'`, which asserts via
//! `tester.binding.transientCallbackCount` — a Flutter scheduler introspection
//! FLUI's harness has no equivalent of) is ported as two inline unit tests,
//! `did_update_view_restarts_only_on_a_genuine_target_change` and
//! `did_update_view_restarts_when_any_single_property_changes`, in
//! `crates/flui-widgets/src/animated/animated_container.rs`'s own
//! `#[cfg(test)] mod tests`, using `ImplicitController::status()` — visible
//! within the crate — to assert the controller's run status directly, since
//! that introspection is not part of this integration harness's public
//! surface. The second test is additional (not itself a distinct oracle
//! case): it isolates the `|=`-not-`||` accumulation the module's own code
//! comment calls out — a later property's unchanged retarget must not mask
//! an earlier one's genuine change.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedContainer, SizedBox, VsyncScope};
use parking_lot::Mutex;

use crate::common::{self, lay_out, lay_out_animated, loose, tight};

const RUN: Duration = Duration::from_millis(200);
const FRAME: Duration = Duration::from_millis(20);

/// `AnimatedContainer` on a zero-area surface lays out to zero without
/// panicking.
///
/// Flutter parity: `'AnimatedContainer does not crash at zero area'`
/// (`animated_container_test.dart`, tag `3.44.0`).
#[test]
fn animated_container_does_not_crash_at_zero_area() {
    let mut laid = lay_out(
        SizedBox::shrink().child(AnimatedContainer::new(SizedBox::shrink()).duration(RUN)),
        tight(0.0, 0.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "AnimatedContainer on a zero-area surface must measure 0×0 with no panic"
    );

    // The oracle pumps (`const Duration(milliseconds: 100)` then
    // `pumpAndSettle`) before its assertion; a never-pumped mount would not
    // catch a panic that only surfaces once a frame actually runs.
    laid.pump_for(RUN);
    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "must still measure 0×0 with no panic after a frame runs"
    );
}

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
            AnimatedContainer::new(SizedBox::new(10.0, 10.0))
                .width(width)
                .height(height)
                .duration(RUN),
        )
    }
}

/// Retargeting only ONE property (`height`) while a sibling (`width`) keeps
/// its existing target must leave `width` completely undisturbed — the
/// `OptTween` Some→Some-unchanged branch reports no change for `width`, so it
/// neither restarts nor snaps.
///
/// Flutter parity: adapted from `'Animation rerun'`
/// (`animated_container_test.dart`, tag `3.44.0`), whose final act changes
/// `height` (200 → 100) while `width` stays at 200 and asserts `width` is
/// pinned exactly throughout while `height` genuinely animates; the earlier
/// portion of that oracle test (retargeting width AND height together,
/// mid-flight bounds, exact settle) is already covered by this suite's
/// `implicit_animations_test.rs::animated_container_width_and_height_stay_in_lockstep_progress`.
#[test]
fn animated_container_retargets_one_property_while_sibling_holds_steady() {
    let vsync = Vsync::new();
    let width = Arc::new(Mutex::new(100.0));
    let height = Arc::new(Mutex::new(100.0));
    let probe = SizeProbe {
        vsync: vsync.clone(),
        width: Arc::clone(&width),
        height: Arc::clone(&height),
    };
    let mut laid = lay_out_animated(probe, loose(400.0), vsync);

    let dims = |laid: &common::LaidOut| -> (f32, f32) {
        let s = laid.size(laid.current_root());
        (s.width.get(), s.height.get())
    };

    // Settle at the initial 100×100 (no motion).
    assert_eq!(dims(&laid), (100.0, 100.0));

    // Retarget height only (100 -> 200); width keeps the same target (100).
    *height.lock() = 200.0;
    laid.pump();
    laid.pump_for(Duration::ZERO); // detection frame: anchors the fresh run

    for _ in 0..3 {
        laid.pump_for(FRAME);
        let (w, _h) = dims(&laid);
        assert_eq!(
            w, 100.0,
            "width's target did not change, so it must stay pinned at 100 \
             throughout the whole run, got {w}"
        );
    }

    // Drive the run to completion.
    laid.pump_for(RUN);
    assert_eq!(
        dims(&laid),
        (100.0, 200.0),
        "height must reach its new target while width stays exactly where it started"
    );
}
