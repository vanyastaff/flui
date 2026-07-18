//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/animated_align_test.dart`
//! (tag `3.44.0`).
//!
//! Widget → render-object mapping:
//! - `AnimatedAlign` → `Align` (rebuilt via `AnimatedBuilder` each tick) →
//!   `RenderAlign` (`crates/flui-widgets/src/animated/animated_align.rs`).
//!
//! Ported: 1 of 7 oracle cases, plus 1 additional (non-oracle) case that
//! fills a coverage gap this suite otherwise had for `AnimatedAlign`
//! entirely (no prior test in the parity suite exercised this widget).
//! - `'AnimatedAlign does not crash at zero area'`
//! - (additional) `animated_align_animates_child_offset_over_duration` — no
//!   single upstream test isolates "the child's position genuinely
//!   interpolates mid-flight and lands exactly on target", the same
//!   architectural-contract gap the module preamble in
//!   `implicit_animations_test.rs` documents for `AnimatedContainer`'s
//!   lockstep guarantee.
//!
//! Out of scope: 6 of 7 — a genuine, worth-flagging capability gap, not a
//! judgment call per case:
//! - `'AnimatedAlign.debugFillProperties'` — no Debug-format parity contract
//!   in this harness (also true of every other `debugFillProperties` case in
//!   this family).
//! - `'AnimatedAlign alignment visual-to-directional animation'` —
//!   `AnimatedAlign::alignment` is a plain `Alignment`, never
//!   `AlignmentGeometry`; there is no `Directionality`-resolved path to
//!   exercise (same family-wide gap as `AnimatedContainer`/`AnimatedPadding`).
//! - `'AnimatedAlign widthFactor'`, `'AnimatedAlign heightFactor'`,
//!   `'AnimatedAlign null height factor'`, `'AnimatedAlign null widthFactor'`
//!   — **`AnimatedAlign` does not expose `width_factor`/`height_factor` at
//!   all**, unlike the plain (non-animated) `Align` widget, which does
//!   (`crates/flui-widgets/src/layout/align.rs`:
//!   `Align::width_factor`/`Align::height_factor`). This is a real,
//!   `AnimatedAlign`-specific capability gap versus its own non-animated
//!   sibling, not shared by the rest of the family — flagged here rather than
//!   silently skipped.

use flui_animation::Vsync;
use flui_types::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedAlign, SizedBox, VsyncScope};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{self, lay_out, lay_out_animated, tight};

const RUN: Duration = Duration::from_millis(200);

/// `AnimatedAlign` on a zero-area surface lays out to zero without panicking.
///
/// Flutter parity: `'AnimatedAlign does not crash at zero area'`
/// (`animated_align_test.dart`, tag `3.44.0`).
#[test]
fn animated_align_does_not_crash_at_zero_area() {
    let laid = lay_out(
        SizedBox::shrink()
            .child(AnimatedAlign::new(Alignment::BOTTOM_CENTER, SizedBox::shrink()).duration(RUN)),
        tight(0.0, 0.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "AnimatedAlign on a zero-area surface must measure 0×0 with no panic"
    );
}

#[derive(Clone, StatefulView)]
struct AlignProbe {
    vsync: Vsync,
    alignment: Arc<Mutex<Alignment>>,
}

struct AlignProbeState {
    vsync: Vsync,
    alignment: Arc<Mutex<Alignment>>,
}

impl StatefulView for AlignProbe {
    type State = AlignProbeState;

    fn create_state(&self) -> Self::State {
        AlignProbeState {
            vsync: self.vsync.clone(),
            alignment: Arc::clone(&self.alignment),
        }
    }
}

impl ViewState<AlignProbe> for AlignProbeState {
    fn build(&self, _view: &AlignProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let alignment = *self.alignment.lock();
        VsyncScope::new(
            self.vsync.clone(),
            AnimatedAlign::new(alignment, SizedBox::new(20.0, 20.0)).duration(RUN),
        )
    }
}

/// `AnimatedAlign` genuinely interpolates its child's position mid-flight and
/// lands exactly on the new alignment at completion — the same lockstep /
/// exact-settle contract `implicit_animations_test.rs` pins for
/// `AnimatedContainer`, ported here for `AnimatedAlign` since no prior test in
/// this suite exercised the widget at all.
///
/// Flutter parity: no single upstream test isolates this (it holds
/// structurally, by construction, from `AnimatedWidgetBaseState`'s shared
/// controller — see `implicit_animations.dart`); this is the same
/// "architectural contract made explicit" rationale the sibling
/// `AnimatedContainer` lockstep test documents.
#[test]
fn animated_align_animates_child_offset_over_duration() {
    let vsync = Vsync::new();
    let alignment = Arc::new(Mutex::new(Alignment::TOP_LEFT));
    let probe = AlignProbe {
        vsync: vsync.clone(),
        alignment: Arc::clone(&alignment),
    };
    // A 100×100 host: TOP_LEFT puts the 20×20 child at (0, 0);
    // BOTTOM_RIGHT puts it at (80, 80).
    let mut laid = lay_out_animated(probe, tight(100.0, 100.0), vsync);

    let child_offset = |laid: &common::LaidOut| -> (f32, f32) {
        let child = laid.only_child(laid.current_root());
        let offset = laid.offset(child);
        (offset.dx.get(), offset.dy.get())
    };

    assert_eq!(
        child_offset(&laid),
        (0.0, 0.0),
        "first build sits at the initial alignment with no motion"
    );

    *alignment.lock() = Alignment::BOTTOM_RIGHT;
    laid.pump();
    laid.pump_for(Duration::ZERO); // detection frame: anchors the fresh run
    laid.pump_for(RUN / 2);

    let (mid_x, mid_y) = child_offset(&laid);
    assert!(
        mid_x > 1.0 && mid_x < 79.0 && mid_y > 1.0 && mid_y < 79.0,
        "halfway through the run the child must be strictly between its \
         start and target offsets, got ({mid_x}, {mid_y})"
    );

    laid.pump_for(RUN / 2);
    let (end_x, end_y) = child_offset(&laid);
    assert!(
        (end_x - 80.0).abs() < 1.0 && (end_y - 80.0).abs() < 1.0,
        "the run must end exactly at the new alignment's offset, got ({end_x}, {end_y})"
    );
}
