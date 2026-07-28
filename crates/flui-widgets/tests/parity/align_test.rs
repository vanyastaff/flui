//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/align_test.dart` (tag
//! `3.44.0`, 6 cases).
//!
//! Ported cases (5 upstream names, 5 Rust tests) plus 1 delta port
//! ([`a_size_factor_applies_only_to_its_own_axis`], 6 Rust tests total):
//! - `'Align smoke test'` — [`align_smoke_test_survives_root_swaps`]. Upstream
//!   pumps five trees in a row and asserts nothing; what it actually guards is
//!   that each swap reconciles without panicking, including swapping a
//!   childless `Align` in and out. The two `AlignmentDirectional` legs are
//!   dropped (see *Framework gap*), leaving three; the ported version keeps the
//!   remaining swaps in the same order and adds a geometry assertion at the end
//!   so the test can fail for a reason other than a panic.
//! - `'Align control test (LTR)'` —
//!   [`align_places_a_child_at_the_top_left_under_ltr`]. Both of upstream's
//!   legs assert the identical `dx` pair (`0.0`, `100.0`): the first through
//!   `AlignmentDirectional.topStart` resolved under LTR, the second through the
//!   physical `Alignment.topLeft`. Only the physical leg has a port target, and
//!   under LTR the two are the same alignment by definition, so no assertion is
//!   lost — only the proof that the *resolution* happens.
//! - `'Align control test (RTL)'` —
//!   [`align_places_a_child_at_the_top_right_when_start_resolves_rtl`].
//! - `'Align widthFactor'` — [`align_width_factor_shrinks_to_a_fraction`].
//! - `'Align heightFactor'` — [`align_height_factor_shrinks_to_a_fraction`].
//!
//! Out of scope (1 case): `'Shrink wraps in finite space'` — asserts an `Align`
//! inside a `SingleChildScrollView` takes the viewport's cross-axis width and
//! its child's main-axis height. FLUI's `SingleChildScrollView` cannot host it:
//! see `scrollable_test.rs`'s own note that the scroll family's child slot is
//! hardwired and does not accept an arbitrary box child. Not an `Align`
//! behavior gap — the case tests `Align` under unbounded constraints, which
//! [`align_width_factor_shrinks_to_a_fraction`] exercises from the other
//! direction (a `Row` giving unbounded width).
//!
//! Framework gap (2 legs folded into ported cases rather than double-counted;
//! no case is lost to it): `Align::new` takes a physical
//! [`Alignment`](flui_types::Alignment), so a directional alignment cannot be
//! expressed at the widget surface and the ambient direction is never read.
//! That costs the `AlignmentDirectional` leg of `'Align smoke test'` and of
//! `'control test (LTR)'` — under LTR the latter's two legs are the same
//! alignment by definition, so no assertion is lost, only the proof that the
//! *resolution* happens.
//!
//! `'Align control test (RTL)'` is ported rather than dropped, using the
//! technique the rest of this corpus already uses for the same gap (see
//! `fractionally_sized_box_test.rs` and `overflow_box_test.rs`): resolve the
//! directional alignment at the call site with
//! `AlignmentDirectional::resolve(false)` — the same resolution the oracle's
//! build phase performs internally — and assert the identical resulting
//! position. That proves the resolution math and `Align`'s placement agree
//! with the oracle; what it does not prove is that a `Directionality`
//! ancestor would be consulted, since no widget-surface path reads one.
//!
//! The gap is narrower than "no directional support": every part it needs
//! already exists and is unwired. `AlignmentGeometry`/`AlignmentDirectional`
//! and `AlignmentGeometry::resolve(is_ltr)` live in
//! `flui_types::layout::alignment`, and
//! `flui_widgets::localization::Directionality` is a real inherited widget
//! with `of`/`maybe_of` — `Dismissible` already reads it
//! (`Directionality::maybe_of(ctx)`). What is missing is only that `Align`
//! neither accepts an `AlignmentGeometry` nor consults the ambient direction.
//! `fitted_box_test.rs` and `transform_test.rs` record the same unwiring for
//! their own widgets.
//!
//! Denominator: 5 ported + 1 out of scope = 6.
//!
//! Widget → render-object mapping: `Align` → `RenderAlign`
//! (`crates/flui-objects/src/layout/align.rs`).

use flui_types::Alignment;
use flui_types::layout::AlignmentDirectional;
use flui_widgets::{Align, Column, Row, SizedBox, column, row};

use crate::common::size;
use crate::harness;

/// Flutter parity: `align_test.dart` `'Align smoke test'` (3.44.0).
///
/// Upstream asserts nothing — it pumps five trees in sequence, so what it
/// guards is that each root swap reconciles without panicking, including
/// swapping a childless `Align` in and then back out. That shape is worth
/// keeping: an `Align` whose child disappears must re-layout as a childless
/// box rather than carry stale child state.
///
/// A test that can only fail by panicking is a weak test, so this adds a
/// closing geometry assertion. Without it, an `Align` that silently kept its
/// previous child's size across the swaps would still "pass".
#[test]
fn align_smoke_test_survives_root_swaps() {
    let mut laid = harness::pump_widget(
        Align::new(Alignment::new(0.5, 0.5)).child(SizedBox::new(20.0, 20.0)),
        harness::screen(),
    );

    laid.pump_widget(Align::new(Alignment::CENTER).child(SizedBox::new(20.0, 20.0)));
    laid.pump_widget(Align::new(Alignment::TOP_LEFT));
    laid.pump_widget(Align::new(Alignment::TOP_LEFT).child(SizedBox::new(30.0, 40.0)));

    // With no size factors the box expands to fill its constraints regardless
    // of alignment, and the child keeps its own size — the state a stale
    // child-size cache would break.
    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(laid.size(align), size(800.0, 600.0));
    assert_eq!(laid.size(laid.only_child(align)), size(30.0, 40.0));
}

/// Flutter parity: `align_test.dart` `'Align control test (LTR)'` (3.44.0) —
/// the physical-`Alignment` leg.
///
/// `Alignment::TOP_LEFT` puts the child's left edge at the box's left edge, so
/// a 100-wide child spans `dx` 0..100. Upstream asserts exactly that pair
/// through `getTopLeft`/`getBottomRight`.
#[test]
fn align_places_a_child_at_the_top_left_under_ltr() {
    let laid = harness::pump_widget(
        Align::new(Alignment::TOP_LEFT).child(SizedBox::new(100.0, 80.0)),
        harness::screen(),
    );

    let align = laid.find_by_render_type("RenderAlign");
    let child = laid.only_child(align);
    let left = laid.absolute_offset(child).dx.get();
    let right = left + laid.size(child).width.get();

    assert_eq!(
        left, 0.0,
        "TOP_LEFT puts the child's left edge at the origin"
    );
    assert_eq!(right, 100.0, "and a 100-wide child therefore ends at 100");
}

/// Flutter parity: `align_test.dart` `'Align widthFactor'` (3.44.0).
///
/// A `width_factor` makes the box size to `factor × child width` instead of
/// filling its constraints — so `0.5` over a 100-wide child gives 50. The
/// `Row` is what makes the assertion meaningful: it hands the `Align`
/// unbounded width, so a box ignoring the factor would expand instead of
/// shrinking, and 50 could not appear by accident.
#[test]
fn align_width_factor_shrinks_to_a_fraction() {
    let laid = harness::pump_widget(
        Row::new(row![
            Align::new(Alignment::CENTER)
                .width_factor(0.5)
                .child(SizedBox::new(100.0, 100.0))
        ]),
        harness::screen(),
    );

    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(laid.size(align).width.get(), 50.0);
}

/// Flutter parity: `align_test.dart` `'Align heightFactor'` (3.44.0).
///
/// The transposed case: a `Column` hands the `Align` unbounded height, and
/// `height_factor: 0.5` over a 100-tall child gives 50.
#[test]
fn align_height_factor_shrinks_to_a_fraction() {
    let laid = harness::pump_widget(
        Column::new(column![
            Align::new(Alignment::CENTER)
                .height_factor(0.5)
                .child(SizedBox::new(100.0, 100.0))
        ]),
        harness::screen(),
    );

    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(laid.size(align).height.get(), 50.0);
}

/// Not an upstream case — a delta port covering the half of `Align`'s sizing
/// contract upstream's two factor cases leave untested.
///
/// `'Align widthFactor'` and `'Align heightFactor'` each set one factor and
/// assert one axis. Neither pins what the *other* axis does, so a
/// `RenderAlign` that applied a set factor to both axes would satisfy both
/// upstream cases.
///
/// The oracle's rule (`RenderPositionedBox.performLayout`,
/// `rendering/shifted_box.dart`, 3.44.0) is per-axis: an axis shrink-wraps iff
/// its factor is set *or* its incoming constraint is unbounded, and the
/// result is then `constrain`ed. So inside a `Row` — unbounded width, bounded
/// height — a `width_factor` of `0.5` over a 100-wide child gives 50, while
/// height, having no factor and a bounded constraint, fills.
///
/// Note the constraint step is not decoration: under the tight screen
/// constraints the other tests use, `constrain(50)` is 800, and the factor is
/// unobservable. That is correct oracle behavior, not a missing shrink — which
/// is why this case has to be posed inside a `Row`.
#[test]
fn a_size_factor_applies_only_to_its_own_axis() {
    let laid = harness::pump_widget(
        Row::new(row![
            Align::new(Alignment::CENTER)
                .width_factor(0.5)
                .child(SizedBox::new(100.0, 100.0))
        ]),
        harness::screen(),
    );

    let align = laid.find_by_render_type("RenderAlign");
    assert_eq!(
        laid.size(align),
        size(50.0, 600.0),
        "width shrink-wraps to factor x child; height has no factor and a \
         bounded constraint, so it fills"
    );
}

/// Flutter parity: `align_test.dart` `'Align control test (RTL)'` (3.44.0).
///
/// Upstream places a 100×80 child with `AlignmentDirectional.topStart` under
/// `TextDirection.rtl` and asserts it spans `dx` 700..800 — the *right* edge of
/// an 800-wide screen, because `start` is the right under RTL.
///
/// `Align` takes a resolved [`Alignment`] and never reads an ambient
/// direction, so the resolution is performed at the call site with
/// `AlignmentDirectional::resolve(false)` — the same step the oracle's build
/// phase performs internally. `fractionally_sized_box_test.rs` and
/// `overflow_box_test.rs` port their own RTL cases exactly this way.
///
/// What this proves: `AlignmentDirectional::resolve`'s RTL math and `Align`'s
/// placement agree with the oracle, on the oracle's own numbers. What it does
/// not prove: that a `Directionality` ancestor would be consulted — no
/// widget-surface path reads one. Resolving to `TOP_RIGHT` by hand instead
/// would assert placement while quietly skipping the resolution, so the
/// directional constant is deliberately kept in the test.
#[test]
fn align_places_a_child_at_the_top_right_when_start_resolves_rtl() {
    let resolved = AlignmentDirectional::TOP_START.resolve(false);
    assert_eq!(
        resolved,
        Alignment::TOP_RIGHT,
        "under RTL, start is the right edge — the premise the placement below tests"
    );

    let laid = harness::pump_widget(
        Align::new(resolved).child(SizedBox::new(100.0, 80.0)),
        harness::screen(),
    );

    let align = laid.find_by_render_type("RenderAlign");
    let child = laid.only_child(align);
    let left = laid.absolute_offset(child).dx.get();
    let right = left + laid.size(child).width.get();

    assert_eq!(left, 700.0, "the oracle's getTopLeft().dx");
    assert_eq!(right, 800.0, "the oracle's getBottomRight().dx");
}
