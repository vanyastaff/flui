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
//! Out of scope: 2 of 7 —
//! - `'AnimatedAlign.debugFillProperties'` — no Debug-format parity contract
//!   in this harness (also true of every other `debugFillProperties` case in
//!   this family).
//! - `'AnimatedAlign alignment visual-to-directional animation'` —
//!   `AnimatedAlign::alignment` is a plain `Alignment`, never
//!   `AlignmentGeometry`; there is no `Directionality`-resolved path to
//!   exercise (same family-wide gap as `AnimatedContainer`/`AnimatedPadding`).
//!
//! The four size-factor cases were listed here as blocked on
//! `AnimatedAlign` not exposing `width_factor`/`height_factor` at all. It
//! exposes both now, animated through `OptTween<f32>` the way the rest of the
//! implicitly-animated family carries its optional properties, so they are
//! ported below:
//! - `'AnimatedAlign widthFactor'` — [`animated_align_width_factor`].
//! - `'AnimatedAlign heightFactor'` — [`animated_align_height_factor`].
//! - `'AnimatedAlign null height factor'`, `'AnimatedAlign null widthFactor'` —
//!   [`animated_align_leaves_an_unset_factor_filling_its_axis`], which covers
//!   both: each upstream case sets neither factor and asserts a 100x100
//!   result, differing only in which enclosing box supplies the unbounded
//!   axis.

use flui_animation::Vsync;
use flui_types::Alignment;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::{AnimatedAlign, Column, Row, SizedBox, VsyncScope, column, row};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{self, lay_out, lay_out_animated, tight};

const RUN: Duration = Duration::from_millis(200);

/// `AnimatedAlign` on a zero-area surface lays out to zero without
/// panicking — checked at mount AND after a frame runs (the oracle's own
/// case pumps via `pumpAndSettle` before asserting; a never-pumped mount
/// would not catch a panic that only surfaces once a frame actually runs).
///
/// Flutter parity: `'AnimatedAlign does not crash at zero area'`
/// (`animated_align_test.dart`, tag `3.44.0`).
#[test]
fn animated_align_does_not_crash_at_zero_area() {
    let mut laid = lay_out(
        SizedBox::shrink()
            .child(AnimatedAlign::new(Alignment::BOTTOM_CENTER, SizedBox::shrink()).duration(RUN)),
        tight(0.0, 0.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "AnimatedAlign on a zero-area surface must measure 0×0 with no panic"
    );

    laid.pump_for(RUN);
    assert_eq!(
        laid.size(laid.current_root()),
        common::size(0.0, 0.0),
        "must still measure 0×0 with no panic after a frame runs"
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
    assert_eq!(
        child_offset(&laid),
        (80.0, 80.0),
        "the run must end exactly at the new alignment's offset"
    );
}

/// Flutter parity: `animated_align_test.dart` `'AnimatedAlign widthFactor'`
/// (3.44.0) — a `width_factor` of `0.5` over a 100-wide child sizes the box to
/// 50.
///
/// The `Row` is load-bearing, not scenery: it hands the `AnimatedAlign`
/// unbounded width. Under the harness's tight screen constraints the factor is
/// unobservable, because `RenderPositionedBox` shrink-wraps and then
/// `constrain`s — `constrain(50)` against a tight 800 is 800. The same trap the
/// plain `Align` port documents.
#[test]
fn animated_align_width_factor() {
    let laid = lay_out(
        Row::new(row![
            AnimatedAlign::new(Alignment::CENTER, SizedBox::new(100.0, 100.0))
                .width_factor(0.5)
                .duration(RUN)
        ]),
        tight(800.0, 600.0),
    );

    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(laid.size(align).width.get(), 50.0);
}

/// Flutter parity: `animated_align_test.dart` `'AnimatedAlign heightFactor'`
/// (3.44.0) — the transposed case, with a `Column` supplying the unbounded
/// axis.
#[test]
fn animated_align_height_factor() {
    let laid = lay_out(
        Column::new(column![
            AnimatedAlign::new(Alignment::CENTER, SizedBox::new(100.0, 100.0))
                .height_factor(0.5)
                .duration(RUN)
        ]),
        tight(800.0, 600.0),
    );

    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(laid.size(align).height.get(), 50.0);
}

/// Flutter parity: `animated_align_test.dart` `'AnimatedAlign null height
/// factor'` and `'AnimatedAlign null widthFactor'` (3.44.0), both covered here.
///
/// The two upstream cases are the same assertion from opposite axes: neither
/// sets a factor, both expect the box to measure exactly the child (100x100),
/// and they differ only in which enclosing widget supplies the unbounded axis.
///
/// An unset factor must reach `Align` as *unset*, not as `1.0`. The two are
/// indistinguishable here — `1.0 x 100` is also 100 — so this test alone cannot
/// tell them apart, and says so rather than implying otherwise. What separates
/// them is the *bounded* axis: with no factor the box fills its constraint
/// there, which is asserted below and which a blanket `1.0` default would
/// break.
#[test]
fn animated_align_leaves_an_unset_factor_filling_its_axis() {
    let laid = lay_out(
        Row::new(row![
            AnimatedAlign::new(Alignment::CENTER, SizedBox::new(100.0, 100.0)).duration(RUN)
        ]),
        tight(800.0, 600.0),
    );

    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(
        laid.size(align),
        common::size(100.0, 600.0),
        "unbounded width shrink-wraps to the child (the oracle's 100); the \
         bounded height has no factor and therefore fills, which is what \
         distinguishes an unset factor from a 1.0 one"
    );
}

#[derive(Clone, StatefulView)]
struct FactorProbe {
    vsync: Vsync,
    factor: Arc<Mutex<f32>>,
}

struct FactorProbeState {
    vsync: Vsync,
    factor: Arc<Mutex<f32>>,
}

impl StatefulView for FactorProbe {
    type State = FactorProbeState;

    fn create_state(&self) -> Self::State {
        FactorProbeState {
            vsync: self.vsync.clone(),
            factor: Arc::clone(&self.factor),
        }
    }
}

impl ViewState<FactorProbe> for FactorProbeState {
    fn build(&self, _view: &FactorProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        let factor = *self.factor.lock();
        VsyncScope::new(
            self.vsync.clone(),
            Row::new(row![
                AnimatedAlign::new(Alignment::CENTER, SizedBox::new(100.0, 100.0))
                    .width_factor(factor)
                    .duration(RUN)
            ]),
        )
    }
}

/// A changed size factor **animates**; it does not snap.
///
/// This is the case that separates a faithful port from a pass-through. The
/// three static factor cases above each lay out once, so a `width_factor` that
/// merely forwarded its value to `Align` would satisfy all of them. The oracle
/// tweens the factor alongside the alignment
/// (`_AnimatedAlignState._widthFactorTween`, `implicit_animations.dart`), and
/// only a mid-run sample can tell the two apart.
///
/// Red-check: make `AnimatedAlign` forward `width_factor` straight to `Align`
/// instead of holding it in an `OptTween` — the halfway assertion then reads
/// the target width 100 instead of a value strictly between 50 and 100.
#[test]
fn animated_align_animates_a_changed_size_factor() {
    let vsync = Vsync::new();
    let factor = Arc::new(Mutex::new(0.5));
    let probe = FactorProbe {
        vsync: vsync.clone(),
        factor: Arc::clone(&factor),
    };
    let mut laid = lay_out_animated(probe, tight(800.0, 600.0), vsync);

    let width = |laid: &common::LaidOut| -> f32 {
        laid.size(laid.find_by_render_type("RenderAlign"))
            .width
            .get()
    };

    assert_eq!(width(&laid), 50.0, "first build sits at the initial factor");

    *factor.lock() = 1.0;
    laid.pump();
    laid.pump_for(Duration::ZERO); // detection frame: anchors the fresh run
    laid.pump_for(RUN / 2);

    let mid = width(&laid);
    assert!(
        mid > 51.0 && mid < 99.0,
        "halfway through the run the box must be strictly between the old and \
         new factor widths; got {mid} — a value of 100 means the factor snapped \
         rather than animating"
    );

    laid.pump_for(RUN / 2);
    assert_eq!(
        width(&laid),
        100.0,
        "the run ends exactly at the new factor"
    );
}
