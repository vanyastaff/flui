//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/scrollbar_test.dart`
//! (tag `3.44.0`).
//!
//! FLUI's `Scrollbar` (`crates/flui-widgets/src/scroll/scrollbar.rs`) is an
//! intentionally thin v1: a single `StatelessView` that overlays a
//! proportional thumb (via `Stack` + `Positioned` + `GestureDetector`) on the
//! trailing edge, driven by `ScrollController::thumb_fraction()` /
//! `thumb_offset_fraction()`. There is no `RawScrollbar`, no
//! `ScrollbarPainter`, no `ScrollbarTheme`, no track widget, and no
//! fade/hover/orientation machinery — so most of the ~3800-line oracle file
//! (which exercises `RawScrollbar`'s full configuration surface) does not
//! apply. What follows is the honest split.
//!
//! Ported (this file):
//! - `'When scrolling normally (no overscrolling), the size of the scrollbar
//!   stays the same, and it scrolls evenly'` + `'should scroll towards the
//!   right direction'` (both formula-checking tests collapsed into one FLUI
//!   case that also exercises a non-zero `min_scroll_extent`, which neither
//!   upstream test does alone) →
//!   [`thumb_geometry_matches_the_proportional_formula_and_advances_monotonically`].
//! - `'Scrollbar is not smaller than minLength with large scroll views, if
//!   minLength is small'` (adapted: FLUI's minimum thumb extent is the fixed
//!   `MIN_THUMB_PX = 18.0` constant, not a configurable `minLength`, so this
//!   pins the same clamp at FLUI's one fixed value rather than sweeping
//!   several `minLength`s) →
//!   [`thumb_extent_clamps_to_the_minimum_when_the_scroll_view_is_very_large`].
//!   Also stands in for `'minThumbLength property of RawScrollbar is
//!   respected'`, which exercises the identical clamp through a
//!   caller-supplied `minThumbLength` FLUI has no equivalent builder for.
//! - `'Scrollbar thumb cannot be dragged into overscroll if the physics do
//!   not allow'` (the min-extent side; the max-extent side is already
//!   covered — see below) →
//!   [`scrollbar_thumb_drag_clamps_at_min_scroll_extent`].
//! - `'Scrollbar gestures disabled when maxScrollExtent == minScrollExtent'` →
//!   [`no_thumb_renders_and_gestures_are_inert_when_content_exactly_fits`].
//! - `'Scrollbar gestures enabled when maxScrollExtent > minScrollExtent by
//!   any amount'` (adapted: FLUI has no negative-`minScrollExtent`-via-
//!   `CustomScrollView.center` construction in this corpus, so this uses
//!   `ScrollController::update_dimensions`'s direct extent write instead —
//!   the dropped assertion is the sliver-tree construction path itself, not
//!   the "any positive scroll_extent keeps the thumb interactive" behavior
//!   under test) →
//!   [`scrollbar_thumb_is_interactive_when_scroll_extent_is_a_tiny_positive_amount`].
//! - `'hit test'` (regression for flutter/flutter#99324), adapted as a
//!   **documented divergence**: Flutter's `RawScrollbar` (with
//!   `trackVisibility: true`) intercepts pointer events across the whole
//!   track column, so a tap between the thumb and the content still misses
//!   the child. FLUI's `Scrollbar` only wraps the thumb's own `Positioned`
//!   rect in a `GestureDetector` — there is no track widget at all (see this
//!   crate's `scrollbar.rs` module doc, "Deferred (v1)") — so a tap in the
//!   track area that isn't on the thumb falls through to the content
//!   underneath. Both the "tap on thumb blocks content" half (undiverged)
//!   and the "tap on track reaches content" half (the divergence, pinned
//!   rather than silently left untested) are asserted →
//!   [`tapping_the_thumb_blocks_content_but_the_track_area_passes_through`].
//! - `'Do not crash when resize from scrollable to non-scrollable.'`
//!   (regression for flutter/flutter#92262) →
//!   [`resizing_to_a_non_scrollable_extent_hides_the_thumb_and_stale_taps_are_inert`].
//!
//! Not ported from any single upstream test, but a real correctness gap
//! found while reading the "thumb resizes gradually on overscroll" oracle
//! against FLUI's implementation — the module doc's own inline comment
//! ("Clamp thumb top so thumb never overflows the track") is unsound once
//! `pixels` sits outside `[min_scroll_extent, max_scroll_extent]` (reachable
//! any time a `Scrollable` using `BouncingScrollPhysics` shares this
//! controller and is mid-overscroll — the `Scrollbar`'s own thumb-drag
//! already clamps, but an external physics-driven overscroll on the SAME
//! controller does not go through that clamp at all). Fixed by clamping
//! `thumb_top` to `[0.0, available_track]` in `scrollbar.rs`. This is
//! narrower than Flutter's behavior (which additionally *shrinks* the thumb
//! toward `minOverscrollLength` during overscroll — out of scope, no such
//! shrink model exists) but closes the actual overflow bug:
//! - [`thumb_top_never_extends_past_the_bottom_of_the_track_when_pixels_overshoots_max`]
//! - [`thumb_top_never_extends_above_the_top_of_the_track_when_pixels_undershoots_min`]
//!
//! Adequately covered elsewhere, not re-ported to avoid duplication:
//! - `'Scrollbar thumb can be dragged'` (the max-extent, forward-drag half)
//!   and the max-extent half of `'Scrollbar thumb cannot be dragged into
//!   overscroll if the physics do not allow'` — both already exercised by
//!   `tests/scroll.rs`'s `scrollbar_thumb_drag_moves_scroll_offset_proportionally`
//!   and `scrollbar_thumb_drag_clamps_at_max_scroll_extent`.
//!
//! Out of scope — no corresponding FLUI feature, so there is nothing to
//! adapt a geometry assertion onto:
//! - `mainAxisMargin is respected'`, `'crossAxisMargin & text direction are
//!   respected'`, `'trackRadius and radius is respected'`, the `shape`
//!   property tests (`CircleBorder`/`RoundedRectangleBorder`/
//!   `BeveledRectangleBorder`), `'RawScrollbar.padding replaces
//!   MediaQueryData.padding'`, `'Track offset respects MediaQuery padding'`,
//!   `'RawScrollbar correctly assigns colors'`, `'trackRadius and radius
//!   properties of RawScrollbar can draw RoundedRectangularRect'` —
//!   `Scrollbar` only exposes `thumb_color`/`thumb_width`; no margins, radii,
//!   shape, or padding customization exists (module doc's own "Deferred
//!   (v1)").
//! - `'scrollbarOrientation are respected'`,
//!   `'scrollbarOrientation default values are correct'`,
//!   `'ScrollbarPainter asserts if scrollbarOrientation is used with wrong
//!   axisDirection'`, `'Drag horizontal and vertical scrollbars'`,
//!   `'ScrollbarPainter asserts if no TextDirection has been provided'` —
//!   FLUI's `Scrollbar` is vertical/right-edge-only; no orientation or
//!   `TextDirection` parameter exists (module doc's own "Deferred (v1)":
//!   "Horizontal scrollbar orientation").
//! - `'Scrollbar thumb can be dragged in reverse'` — `Scrollbar` has no
//!   `reverse`/`axisDirection` awareness at all (it reads only
//!   `pixels`/`min_scroll_extent`/`max_scroll_extent`, none of which encode
//!   direction), so pairing it with a `reverse: true`
//!   `SingleChildScrollView` would silently produce a thumb that moves the
//!   wrong way — a real v1 gap, not exercised here because no test
//!   constructs that combination today.
//! - `'thumb resizes gradually on overscroll'`,
//!   `'minOverscrollLength property of RawScrollbar is respected'` — no
//!   overscroll-aware thumb-shrink model exists; FLUI's fix in this file
//!   (see above) keeps the thumb within track bounds during overscroll but
//!   does not shrink it.
//! - `'Scrollbar never goes away until finger lift'`, `'Scrollbar does not
//!   fade away while hovering'`, `'Scrollbar will fade back in when hovering
//!   over known track area'`, `'Scrollbar will show on hover without needing
//!   to scroll first for metrics'` — no fade-in/fade-out animation exists
//!   (module doc's own "Deferred (v1)").
//! - `'Tapping the track area pages the Scroll View'`,
//!   `'Scrollbar asserts that a visible track has a visible thumb'`,
//!   `'Scrollbar track can be drawn'` — no separate track widget/gesture
//!   region exists at all (only the thumb's own rect is interactive).
//! - `'Scrollbar hit test area adjusts for PointerDeviceKind'` — FLUI's
//!   thumb hit region is exactly its painted `Positioned` rect; there is no
//!   device-kind-conditional hit-padding.
//! - `'The bar supports mouse wheel event'`, `'Simultaneous dragging and
//!   pointer scrolling does not cause a crash'` — no pointer-signal
//!   (mouse-wheel/trackpad) input path exists on `Scrollable`/`Scrollbar`
//!   (already noted as a gap in `parity/scrollable_test.rs`'s module doc).
//! - `'notificationPredicate depth test.'`, `'RawScrollbar.thumbVisibility
//!   asserts that a ScrollPosition is attached'`, `'Scrollbars assert on
//!   multiple scroll positions'`, `'Interactive scrollbars should have a
//!   valid scroll controller'`, `'ScrollbarPainter.shouldRepaint returns true
//!   when any of the properties changes'`, `'Skip the ScrollPosition check if
//!   the bar was unmounted'` — all reference `RawScrollbar`/`ScrollbarPainter`
//!   internal machinery (notification-predicate bubbling, an attach/detach
//!   protocol, a separate painter object with its own repaint diffing) that
//!   has no FLUI equivalent; `Scrollbar` is one `StatelessView` with no
//!   painter layer and a 1:1 controller binding (already documented in
//!   `parity/scroll_controller_test.rs`'s module doc).
//! - `'Scrollbar respect the NeverScrollableScrollPhysics physics'`, `'The
//!   scrollable should not stutter when the scroll metrics shrink during
//!   dragging'`, `'The bar can show or hide when the viewport size change'`,
//!   `'The bar can show or hide when the view size change'` — deferred for
//!   volume; they exercise the same `show_thumb`/thumb-drag machinery this
//!   file already covers via
//!   [`no_thumb_renders_and_gestures_are_inert_when_content_exactly_fits`] and
//!   [`resizing_to_a_non_scrollable_extent_hides_the_thumb_and_stale_taps_are_inert`],
//!   just through additional configuration surfaces (`NeverScrollableScrollPhysics`,
//!   `MediaQuery` view-size changes) FLUI's harness does not model.
//!
//! Widget → render-object mapping: `Scrollbar` → `AnimatedBuilder` →
//! `Stack` (`RenderStack`) → content (non-positioned) + thumb
//! `GestureDetector` → `Positioned` → `ColoredBox`. `ColoredBox` is realised
//! as a `DecoratedBox(decoration: BoxDecoration(color: ...))`
//! (`crates/flui-widgets/src/paint/colored_box.rs`'s own doc), so the thumb
//! render node is a `RenderDecoratedBox` — found via `find_by_render_type`,
//! unique in the tree because no other widget in these tests carries a
//! `BoxDecoration`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_rendering::hit_testing::HitTestBehavior;
use flui_widgets::{GestureDetector, ScrollController, Scrollbar, SizedBox};

use crate::common::{lay_out, size, tight};

/// The proportional thumb-size/offset formula
/// (`thumb_fraction = viewport / (viewport + scroll_extent)`,
/// `thumb_top = available_track * thumb_offset_fraction` with
/// `thumb_offset_fraction = (pixels - min_scroll_extent) / scroll_extent` —
/// see that method's doc in `scroll_controller.rs`) must hold across several
/// scroll positions, with the thumb's extent staying constant while only its
/// offset moves, reaching the track's ends EXACTLY (flush, not short of
/// them) at `pixels == min_scroll_extent` / `pixels == max_scroll_extent` —
/// and a non-zero `min_scroll_extent` must be folded into the `(pixels -
/// min_scroll_extent)` term, not just `pixels` alone.
///
/// Flutter parity: `scrollbar_test.dart` `'When scrolling normally (no
/// overscrolling), the size of the scrollbar stays the same, and it scrolls
/// evenly'` — asserts `rect.height` stays
/// `nearEqual(viewportDimension^2 / (viewportDimension + maxExtent))` across
/// a generated list of scroll positions, with `rect.top`'s ratio to `pixels`
/// staying constant. `'should scroll towards the right direction'` —
/// asserts `rect.top`/`rect.bottom` strictly increase as `pixels` increases
/// (using a *negative* `minScrollExtent`, which this case borrows the idea
/// from instead of Flutter's own exhaustive per-viewport-height generative
/// loop). The flush-at-the-end assertion below is this case's own addition,
/// not lifted from either upstream test verbatim — both assert the ratio
/// stays constant and increases monotonically, not the exact endpoint
/// value — but it is what makes the formula's `(pixels - min) / scroll_extent`
/// contract (no extra `(1 - thumb_fraction)` factor) verifiable: with that
/// extra factor still present the thumb would stop `available_track *
/// thumb_fraction` short of the track's far edge instead of reaching it.
///
/// Dropped assertions (both upstream tests): the exhaustive
/// `List.generate` sweep over every viewport-height step (this uses 5
/// representative points); Flutter's `rect.left`/`rect.width` checks (no
/// track/thickness geometry exists here — `thumb_width` IS the whole
/// painted extent, already covered by the exact offset/size equality below).
#[test]
fn thumb_geometry_matches_the_proportional_formula_and_advances_monotonically() {
    // viewport=80, min=-100, max=140 -> scroll_extent=240, content_length=320.
    // thumb_fraction = 80/320 = 0.25 -> thumb_height = 80*0.25 = 20 (>= the
    // 18px minimum, so the min-clamp does not engage here).
    // available_track = 80 - 20 = 60.
    // thumb_top(pixels) = available_track * (pixels - min)/scroll_extent
    //                   = 60 * (pixels + 100)/240 = 0.25 * (pixels + 100)
    let controller = ScrollController::new();
    controller.update_dimensions(80.0, -100.0, 140.0);

    let widget = || {
        Scrollbar::new()
            .controller(controller.clone())
            .child(SizedBox::new(200.0, 80.0))
    };
    let mut laid = lay_out(widget(), tight(200.0, 80.0));

    let expected_top = |pixels: f32| 0.25 * (pixels + 100.0);
    let mut previous_top = f32::NEG_INFINITY;

    for pixels in [-100.0_f32, -40.0, 20.0, 80.0, 140.0] {
        controller.set_pixels(pixels);
        laid.pump();

        let thumb_id = laid.find_by_render_type("RenderDecoratedBox");
        let thumb_size = laid.size(thumb_id);
        assert_eq!(
            thumb_size,
            size(6.0, 20.0),
            "thumb extent must stay constant (20px) regardless of scroll position; \
             pixels={pixels}"
        );
        let top = laid.absolute_offset(thumb_id).dy.get();
        assert!(
            (top - expected_top(pixels)).abs() < 1e-3,
            "thumb_top must match the proportional formula at pixels={pixels}: \
             expected {:.4}, got {top:.4}",
            expected_top(pixels)
        );
        assert!(
            top > previous_top,
            "thumb_top must strictly increase as pixels increases: previous={previous_top:.4}, \
             current={top:.4} (pixels={pixels})"
        );
        previous_top = top;

        // At max_scroll_extent (140.0, the last sample point) the thumb must
        // sit FLUSH with the track's far edge — bottom == viewport_dim
        // exactly, not short of it. A version of the formula that still
        // folds in `(1 - thumb_fraction)` would stop at
        // `available_track * thumb_fraction` = 60 * 0.25 = 15px short here
        // (bottom == 65, not 80).
        if pixels == 140.0 {
            let bottom = top + thumb_size.height.get();
            assert_eq!(
                bottom, 80.0,
                "thumb bottom must be flush with the track end (viewport_dim=80) \
                 at pixels == max_scroll_extent; got {bottom}"
            );
        }
    }
}

/// A very large `scroll_extent` drives `thumb_fraction` toward zero, which
/// would make the naive proportional thumb height sub-pixel — the
/// `MIN_THUMB_PX = 18.0` floor must still apply, at every scroll position
/// (not just the initial one).
///
/// Flutter parity: `scrollbar_test.dart` `'Scrollbar is not smaller than
/// minLength with large scroll views, if minLength is small'` — checks
/// `rect.height >= minLen` both at the initial position and after a
/// `pixels` update. Also stands in for `'minThumbLength property of
/// RawScrollbar is respected'` (same clamp, exercised through a
/// caller-supplied `minThumbLength` FLUI has no builder for — `MIN_THUMB_PX`
/// is a fixed module constant, so this pins the one value FLUI actually
/// uses instead of sweeping several).
///
/// Dropped assertion: Flutter's `minLength`/`minOverscrollLength` are
/// caller-configurable; FLUI's is not, so there is only one clamp value to
/// pin, not a parametrized sweep.
#[test]
fn thumb_extent_clamps_to_the_minimum_when_the_scroll_view_is_very_large() {
    // viewport=100, scroll_extent=100_000 -> thumb_fraction ~= 0.000999,
    // naive thumb_height ~= 0.0999px -- must clamp to 18.0.
    let controller = ScrollController::new();
    controller.update_dimensions(100.0, 0.0, 100_000.0);

    let widget = || {
        Scrollbar::new()
            .controller(controller.clone())
            .child(SizedBox::new(200.0, 100.0))
    };
    let mut laid = lay_out(widget(), tight(200.0, 100.0));

    for pixels in [0.0_f32, 50_000.0, 100_000.0] {
        controller.set_pixels(pixels);
        laid.pump();

        let thumb_id = laid.find_by_render_type("RenderDecoratedBox");
        let height = laid.size(thumb_id).height.get();
        assert_eq!(
            height, 18.0,
            "thumb height must clamp to the 18px minimum even when the proportional \
             formula would produce a sub-pixel extent; pixels={pixels}, got {height}"
        );
    }
}

/// Dragging the thumb up past the top of the track must clamp the scroll
/// offset at `min_scroll_extent` (0.0), symmetric to
/// `tests/scroll.rs`'s `scrollbar_thumb_drag_clamps_at_max_scroll_extent`
/// which already covers the max-extent side.
///
/// Flutter parity: `scrollbar_test.dart` `'Scrollbar thumb cannot be
/// dragged into overscroll if the physics do not allow'` — drags the thumb
/// by a negative `scrollAmount` from `offset == 0.0` and asserts the offset
/// stays at `0.0`. This case starts from a nonzero position (100.0) instead
/// of 0.0 so a passing run must OBSERVE the position actually decrease
/// before the clamp engages — an unmoved 0.0 could not distinguish
/// "clamped" from "the drag never fired" (same reasoning
/// `scrollable_drag_down_at_min_extent_is_clamped_by_physics` in
/// `parity/scrollable_test.rs` uses for its own pre-scroll seed).
///
/// Dropped assertion: Flutter's own paired `paints` thumb-rect checks
/// before/after (no separate track/painter geometry exists here to
/// re-assert; the offset-fraction formula is already pinned by
/// [`thumb_geometry_matches_the_proportional_formula_and_advances_monotonically`]).
#[test]
fn scrollbar_thumb_drag_clamps_at_min_scroll_extent() {
    // viewport=300, scroll_extent=300 -> thumb_fraction=0.5, thumb_height=150,
    // available_track=150. Starting at pixels=100: thumb_top = available_track
    // * (pixels/scroll_extent) = 150 * (100/300) = 50 -> thumb y=[50,200],
    // x=[280,300] (thumb_width=20).
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 300.0);
    controller.set_pixels(100.0);

    let widget = Scrollbar::new()
        .controller(controller.clone())
        .thumb_width(20.0)
        .child(SizedBox::new(300.0, 300.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    assert_eq!(
        controller.pixels(),
        100.0,
        "must start at the seeded position"
    );

    // Every position below stays comfortably within the thumb's ORIGINAL
    // y=[50,200] bounds (no rebuild happens mid-drag, matching the existing
    // max-side test's own precedent), with a wide margin off both edges so
    // hit-testing is unambiguous. Each -20 track-px move maps to
    // content_delta = (-20/150)*300 = -40 (`dP/d(thumb_top) = scroll_extent /
    // available_track`, this file's proportional-formula test).
    //   Down at (290, 170)      -- inside thumb
    //   Move to (290, 150) -20  -- slop-crossing (>18px): on_pan_start (no-op)
    //   Move to (290, 130) -20  -- on_pan_update: proposed = 100 - 40 = 60
    //   Move to (290, 110) -20  -- on_pan_update: proposed = 60 - 40 = 20
    //   Move to (290,  90) -20  -- on_pan_update: proposed = 20 - 40 = -20 -> clamp 0
    scoped.dispatch_pointer_down(290.0, 170.0);
    scoped.dispatch_pointer_move(290.0, 150.0);
    scoped.dispatch_pointer_move(290.0, 130.0);
    scoped.dispatch_pointer_move(290.0, 110.0);
    scoped.dispatch_pointer_move(290.0, 90.0);
    scoped.dispatch_pointer_up(290.0, 90.0);

    assert_eq!(
        controller.pixels(),
        0.0,
        "dragging the thumb up past the top must clamp at min_scroll_extent (0), \
         having demonstrably moved from the seeded 100.0; got {:.2}",
        controller.pixels()
    );
}

/// When content exactly fits the viewport (`max_scroll_extent ==
/// min_scroll_extent`), no thumb should render at all, and pointer events
/// over the (now-empty) trailing edge must not affect the scroll position.
///
/// Flutter parity: `scrollbar_test.dart` `'Scrollbar gestures disabled when
/// maxScrollExtent == minScrollExtent'` — asserts the scrollbar's
/// `RawGestureDetector` has zero registered gestures. FLUI has no separate
/// gesture-detector-with-a-gesture-count to introspect; the analogous,
/// stronger assertion is that no thumb `GestureDetector` is even mounted
/// (`find_all_by_render_type` returns empty) and that dispatching pointer
/// events there is a no-op.
#[test]
fn no_thumb_renders_and_gestures_are_inert_when_content_exactly_fits() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 0.0);

    let widget = Scrollbar::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 300.0));

    let laid = lay_out(widget, tight(300.0, 300.0));

    assert!(
        laid.find_all_by_render_type("RenderDecoratedBox")
            .is_empty(),
        "no thumb should be mounted when the content exactly fits the viewport"
    );

    // Trailing-edge column where a thumb would have painted, had one existed.
    laid.dispatch_pointer_down(297.0, 10.0);
    laid.dispatch_pointer_move(297.0, 60.0);
    laid.dispatch_pointer_up(297.0, 60.0);

    assert_eq!(
        controller.pixels(),
        0.0,
        "pointer events over the (absent) thumb area must not move the scroll offset"
    );
}

/// Even a tiny positive `scroll_extent` (content barely taller than the
/// viewport) must render an interactive thumb — `show_thumb` gates on
/// `fraction < 1.0`, not on some minimum overflow amount.
///
/// Flutter parity: `scrollbar_test.dart` `'Scrollbar gestures enabled when
/// maxScrollExtent > minScrollExtent by any amount'` — constructs a
/// `CustomScrollView` with a negative `minScrollExtent` via a `center`
/// sliver and asserts the scrollbar's gesture detector has registered
/// gestures. This case reaches the same "any positive scroll_extent keeps
/// the thumb interactive" behavior directly through
/// `ScrollController::update_dimensions` (the dropped assertion is the
/// sliver-tree construction path itself, not this behavior).
#[test]
fn scrollbar_thumb_is_interactive_when_scroll_extent_is_a_tiny_positive_amount() {
    // viewport=300, scroll_extent=1.0 -> thumb_fraction = 300/301 ~= 0.9967,
    // thumb_height ~= 299 (no min-clamp), available_track ~= 1.0 -- any drag
    // move maps to a huge content_delta and immediately clamps at max (1.0).
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 1.0);

    let widget = Scrollbar::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 300.0));

    let scoped = lay_out(widget, tight(300.0, 300.0));

    assert!(
        !scoped
            .find_all_by_render_type("RenderDecoratedBox")
            .is_empty(),
        "a thumb must be mounted even for a 1px scroll_extent"
    );
    assert_eq!(controller.pixels(), 0.0);

    // Default thumb_width=6.0 -> x=[294,300]; thumb spans nearly the whole
    // 300px viewport, so any y in range hits it.
    scoped.dispatch_pointer_down(297.0, 50.0);
    scoped.dispatch_pointer_move(297.0, 70.0); // slop-crossing
    scoped.dispatch_pointer_move(297.0, 90.0); // on_pan_update: huge proposed -> clamp
    scoped.dispatch_pointer_up(297.0, 90.0);

    assert_eq!(
        controller.pixels(),
        1.0,
        "dragging the thumb must move the position and clamp at the tiny max_scroll_extent (1.0)"
    );
}

/// Tapping directly on the thumb must block the underlying content's own
/// `GestureDetector` from seeing the tap (the thumb's `Opaque` behavior wins
/// the hit test). Tapping the surrounding "track" column — the same x range
/// the thumb occupies, but a y outside its current bounds — reaches the
/// content underneath, because FLUI mounts no separate track widget at all.
///
/// Flutter parity: `scrollbar_test.dart` `'hit test'` (regression for
/// flutter/flutter#99324) — asserts tapping the thumb AND tapping the track
/// area both leave `onTap` false (only tapping the content area sets it
/// true), because `RawScrollbar`'s track intercepts across its whole
/// column. **Documented divergence**: FLUI's track-area tap reaches the
/// content (`onTap` fires) instead of being blocked, because there is no
/// track widget to intercept it (`scrollbar.rs`'s module doc, "Deferred
/// (v1)": no track visual, no `trackVisibility`). This is pinned as
/// current, intended v1 behavior, not silently left untested.
#[test]
fn tapping_the_thumb_blocks_content_but_the_track_area_passes_through() {
    let tap_count = Arc::new(AtomicUsize::new(0));
    let on_tap_count = Arc::clone(&tap_count);

    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 300.0);

    let content = GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(move || {
            on_tap_count.fetch_add(1, Ordering::SeqCst);
        })
        .child(SizedBox::new(300.0, 300.0));

    let widget = Scrollbar::new()
        .controller(controller)
        .thumb_width(20.0)
        .child(content);

    // At pixels=0: thumb_height=150 -> thumb x=[280,300], y=[0,150].
    let scoped = lay_out(widget, tight(300.0, 300.0));

    // Tap on the thumb (inside x=[280,300], y=[0,150]): must NOT reach content.
    scoped.dispatch_pointer_down(297.0, 10.0);
    scoped.dispatch_pointer_up(297.0, 10.0);
    assert_eq!(
        tap_count.load(Ordering::SeqCst),
        0,
        "a tap on the thumb must be absorbed by its Opaque GestureDetector, \
         not reach the content beneath it"
    );

    // Tap on the track column, but outside the thumb's y range (divergence
    // from Flutter's whole-track intercept — see this test's doc comment).
    scoped.dispatch_pointer_down(297.0, 250.0);
    scoped.dispatch_pointer_up(297.0, 250.0);
    assert_eq!(
        tap_count.load(Ordering::SeqCst),
        1,
        "a tap in the track column but outside the thumb's painted bounds must \
         reach the content — FLUI mounts no separate track hit-test region"
    );

    // Tap on the content area, well away from the scrollbar column entirely.
    scoped.dispatch_pointer_down(100.0, 100.0);
    scoped.dispatch_pointer_up(100.0, 100.0);
    assert_eq!(
        tap_count.load(Ordering::SeqCst),
        2,
        "a tap outside the scrollbar column entirely must reach the content"
    );
}

/// Resizing a scrollable's content down to exactly the viewport size (going
/// from scrollable to non-scrollable) must hide the thumb without
/// panicking, and a stale pointer sequence at the thumb's old on-screen
/// position must not move the scroll offset once the thumb is gone.
///
/// Flutter parity: `scrollbar_test.dart` `'Do not crash when resize from
/// scrollable to non-scrollable.'` (regression for flutter/flutter#92262) —
/// resizes the scrolled content from 700px to 600px (matching a 600px
/// viewport, so `maxScrollExtent` drops to 0) and drags the thumb's old
/// location, asserting only that nothing crashes. This case additionally
/// asserts the thumb is actually gone and the position is provably inert,
/// not just crash-free.
#[test]
fn resizing_to_a_non_scrollable_extent_hides_the_thumb_and_stale_taps_are_inert() {
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 500.0);

    let widget = Scrollbar::new()
        .controller(controller.clone())
        .thumb_width(20.0)
        .child(SizedBox::new(300.0, 300.0));

    let mut scoped = lay_out(widget, tight(300.0, 300.0));

    assert!(
        !scoped
            .find_all_by_render_type("RenderDecoratedBox")
            .is_empty(),
        "the thumb must be present before the resize"
    );

    // Shrink content to exactly the viewport: max_scroll_extent -> 0.
    controller.update_dimensions(300.0, 0.0, 0.0);
    scoped.pump_for(Duration::ZERO);

    assert!(
        scoped
            .find_all_by_render_type("RenderDecoratedBox")
            .is_empty(),
        "the thumb must disappear once max_scroll_extent reaches min_scroll_extent"
    );

    // Stale drag at the thumb's OLD on-screen position (was x=[280,300],
    // y=[0,150] before the resize) must not panic and must not move pixels.
    scoped.dispatch_pointer_down(290.0, 10.0);
    scoped.dispatch_pointer_move(290.0, 60.0);
    scoped.dispatch_pointer_up(290.0, 60.0);

    assert_eq!(
        controller.pixels(),
        0.0,
        "a stale pointer sequence over the thumb's former position must be a no-op \
         once the thumb (and its GestureDetector) no longer exist"
    );
}

/// If `pixels` ever sits ABOVE `max_scroll_extent` (reachable when a
/// `Scrollable` sharing this controller uses `BouncingScrollPhysics` and is
/// mid-overscroll — `Scrollbar`'s own thumb-drag clamps, but an external
/// physics-driven overscroll on the same controller does not route through
/// that clamp), the thumb must not be pushed past the bottom of the track.
///
/// This is the real bug the "thumb resizes gradually on overscroll" oracle
/// test exposed while reading it against FLUI's implementation: the
/// `scrollbar.rs` comment above `available_track` claims "thumb never
/// overflows the track", but the code only bounded `available_track`
/// itself, not `thumb_top` — so an out-of-range `pixels` produced a
/// `thumb_top` past `available_track`, overflowing the track's bottom edge.
#[test]
fn thumb_top_never_extends_past_the_bottom_of_the_track_when_pixels_overshoots_max() {
    // viewport=300, scroll_extent=300 -> thumb_fraction=0.5, thumb_height=150,
    // available_track=150. pixels=900 (3x max) -> offset_fraction =
    // 900/300 = 3.0 -> unclamped thumb_top = 150*3.0 = 450 > 150.
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 300.0);
    controller.set_pixels(900.0);

    let widget = Scrollbar::new()
        .controller(controller)
        .child(SizedBox::new(300.0, 300.0));

    let laid = lay_out(widget, tight(300.0, 300.0));
    let thumb_id = laid.find_by_render_type("RenderDecoratedBox");
    let top = laid.absolute_offset(thumb_id).dy.get();

    assert!(
        top <= 150.0,
        "thumb_top must clamp to available_track (150.0) even when pixels (900.0) \
         sits far past max_scroll_extent (300.0); got {top}"
    );
}

/// Symmetric to the overshoot case above: `pixels` BELOW `min_scroll_extent`
/// must not push the thumb above the top of the track.
#[test]
fn thumb_top_never_extends_above_the_top_of_the_track_when_pixels_undershoots_min() {
    // pixels=-600 -> offset_fraction = -600/300 = -2.0 -> unclamped
    // thumb_top = 150*(-2.0) = -300 < 0.
    let controller = ScrollController::new();
    controller.update_dimensions(300.0, 0.0, 300.0);
    controller.set_pixels(-600.0);

    let widget = Scrollbar::new()
        .controller(controller)
        .child(SizedBox::new(300.0, 300.0));

    let laid = lay_out(widget, tight(300.0, 300.0));
    let thumb_id = laid.find_by_render_type("RenderDecoratedBox");
    let top = laid.absolute_offset(thumb_id).dy.get();

    assert!(
        top >= 0.0,
        "thumb_top must clamp to 0.0 even when pixels (-600.0) sits far below \
         min_scroll_extent (0.0); got {top}"
    );
}
