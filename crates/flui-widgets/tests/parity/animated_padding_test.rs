//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/animated_padding_test.dart`
//! (tag `3.44.0`).
//!
//! Widget → render-object mapping:
//! - `AnimatedPadding` → `Padding` (rebuilt via `AnimatedBuilder` each tick) →
//!   `RenderPadding` (`crates/flui-widgets/src/animated/animated_padding.rs`).
//!
//! Ported: 2 of 4 oracle cases.
//! - `'AnimatedPadding does not crash at zero area'`
//! - `'AnimatedPadding animated padding clamped to positive values'` —
//!   divergence found and fixed, see below.
//!
//! Out of scope: 2 of 4.
//! - `'AnimatedPadding.debugFillProperties'` — asserts Dart's
//!   `hasOneLineDescription` on the widget's `Diagnosticable` output; FLUI's
//!   harness has no equivalent Debug-format parity contract to check against.
//! - `'AnimatedPadding padding visual-to-directional animation'` —
//!   `AnimatedPadding::padding` is a plain `EdgeInsets` (matching `Container`
//!   and `Padding` widget-family-wide, not a gap unique to this widget), never
//!   `EdgeInsetsGeometry`; there is no `Directionality`-resolved padding path
//!   on this widget to exercise.
//!
//! Divergence found + fixed: the oracle's `AnimatedPaddingState.build` clamps
//! the evaluated tween through
//! `.clamp(EdgeInsets.zero, EdgeInsetsGeometry.infinity)`
//! (`implicit_animations.dart` `AnimatedPaddingState.build`) before handing it
//! to `Padding`, specifically so a curve that overshoots below `0`
//! (`Curves.easeInOutBack`, used by this very oracle test) never reaches
//! `RenderPadding` with a negative inset. FLUI's `AnimatedPaddingState::build`
//! evaluated the tween directly with no clamp:
//!
//! ```text
//! RED (pre-fix), animated_padding_clamps_negative_overshoot_to_zero:
//! thread 'animated_padding_test::animated_padding_clamps_negative_overshoot_to_zero'
//! panicked at crates/flui-widgets/tests/parity/animated_padding_test.rs:171:5:
//! an overshooting curve must clamp the evaluated padding to non-negative,
//! got right=-0.6675644px
//! ```
//!
//! Fixed in `animated_padding.rs` (`AnimatedPaddingState::build`) by clamping
//! the evaluated inset with the crate's existing
//! `EdgeInsets::clamp_non_negative()` (`flui-geometry/src/edges.rs`) before
//! constructing `Padding`, matching the oracle's clamp exactly. `AnimatedContainer`
//! does NOT get the same fix — its oracle (`_AnimatedContainerState.build`,
//! `implicit_animations.dart`) evaluates `_padding?.evaluate(animation)`
//! straight into `Container` with no clamp, so an unclamped `AnimatedContainer`
//! padding overshoot is oracle-faithful, not a divergence.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Curves, Vsync};
use flui_geometry::EdgeInsets;
use flui_objects::RenderPadding;
use flui_types::geometry::px;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedPadding, SizedBox, VsyncScope};
use parking_lot::Mutex;

use crate::common::{self, lay_out, lay_out_animated, tight};

/// The evaluated [`EdgeInsets`] of the unique `RenderPadding` node in `laid`.
fn padding_of(laid: &common::LaidOut, id: flui_foundation::RenderId) -> EdgeInsets {
    let owner_handle = laid.pipeline_owner();
    let mut owner = owner_handle.write();
    owner
        .render_tree_mut()
        .get_mut(id)
        .and_then(|node| node.downcast_render_object_mut::<RenderPadding>())
        .expect("render node should be a RenderPadding")
        .padding()
}

/// Probe that rebuilds `AnimatedPadding` with `padding`'s current value on
/// every [`LaidOut::pump`](common::LaidOut::pump) — the same externally
/// mutated `Arc<Mutex<_>>` + `pump()` retarget pattern
/// `implicit_animations_test.rs`'s `SizeProbe` uses.
#[derive(Clone, StatefulView)]
struct PaddingProbe {
    vsync: Vsync,
    padding: Arc<Mutex<EdgeInsets>>,
}

struct PaddingProbeState {
    vsync: Vsync,
    padding: Arc<Mutex<EdgeInsets>>,
}

impl StatefulView for PaddingProbe {
    type State = PaddingProbeState;

    fn create_state(&self) -> Self::State {
        PaddingProbeState {
            vsync: self.vsync.clone(),
            padding: Arc::clone(&self.padding),
        }
    }
}

impl ViewState<PaddingProbe> for PaddingProbeState {
    fn build(&self, _view: &PaddingProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let padding = *self.padding.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedPadding::new(padding, SizedBox::shrink())
                .duration(Duration::from_millis(200))
                .curve(Curves::EaseInOutBack),
        )
    }
}

/// `AnimatedPadding` on a zero-area surface lays out to zero without panicking.
///
/// Flutter parity: `'AnimatedPadding does not crash at zero area'`
/// (`animated_padding_test.dart`, tag `3.44.0`).
#[test]
fn animated_padding_does_not_crash_at_zero_area() {
    let laid = lay_out(
        SizedBox::shrink().child(
            AnimatedPadding::new(EdgeInsets::all(px(1.0)), SizedBox::shrink())
                .duration(Duration::from_millis(200)),
        ),
        tight(0.0, 0.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "AnimatedPadding on a zero-area surface must measure 0×0 with no panic"
    );
}

/// A curve that overshoots below `0` (`Curves::EaseInOutBack`, same as the
/// oracle) must never hand `RenderPadding` a negative inset — the widget
/// clamps the evaluated tween to non-negative before building `Padding`.
///
/// Flutter parity: `'AnimatedPadding animated padding clamped to positive
/// values'` (`animated_padding_test.dart`, tag `3.44.0`) — same duration
/// (200ms), same overshoot curve, and the same 128ms sample point at which
/// the oracle's un-clamped curve value would go negative.
#[test]
fn animated_padding_clamps_negative_overshoot_to_zero() {
    let vsync = Vsync::new();
    let padding = Arc::new(Mutex::new(EdgeInsets::only_right(px(50.0))));
    let probe = PaddingProbe {
        vsync: vsync.clone(),
        padding: Arc::clone(&padding),
    };
    let mut laid = lay_out_animated(probe, tight(800.0, 600.0), vsync);

    let padding_id = laid.find_by_render_type("RenderPadding");
    assert_eq!(
        padding_of(&laid, padding_id).right,
        px(50.0),
        "first build sits at the given padding with no motion"
    );

    // Retarget toward zero — the curve overshoots below 0 partway through.
    *padding.lock() = EdgeInsets::ZERO;
    laid.pump();
    laid.pump_for(Duration::ZERO); // detection frame: anchors the fresh run
    laid.pump_for(Duration::from_millis(128));

    let evaluated = padding_of(&laid, padding_id);
    assert!(
        evaluated.right >= px(0.0),
        "an overshooting curve must clamp the evaluated padding to \
         non-negative, got right={:?}",
        evaluated.right
    );
    assert!(
        evaluated.top >= px(0.0) && evaluated.bottom >= px(0.0) && evaluated.left >= px(0.0),
        "every edge must be clamped, not just the animated one, got {evaluated:?}"
    );
}
