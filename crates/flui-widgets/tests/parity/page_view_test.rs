//! Flutter parity tests — `PageView` (ADR-0037 PR2).
//!
//! Flutter source: `packages/flutter/test/widgets/page_view_test.dart` (tag
//! `3.44.0`, **50** `testWidgets` cases —
//! `awk '/testWidgets\(/{print NR}' test/widgets/page_view_test.dart | wc -l`).
//!
//! ## Scope of this port
//!
//! `crates/flui-widgets/src/scroll/page_view.rs`'s module docs record the
//! exact V1 boundary (eager children, listener-based `on_page_changed`, no
//! `pageSnapping: false`/`reverse`/`padEnds`/`allowImplicitScrolling`/
//! `PageStorage`/`animateToPage`/`viewport_fraction > 1.0` centering). **9
//! tests ported below**, covering the portable core:
//!
//! - `initial_page_lands_on_first_layout` — `initialPage` seeding, closing
//!   PR1's explicitly deferred `_pageToUseOnStartup` gap. Cross-checked
//!   against `'PageController control test'` (line 309).
//! - `resize_keeps_the_page_across_a_viewport_dimension_change` —
//!   `DimensionChangePolicy::KeepFractionalPage` end-to-end through a real
//!   `PageView` resize. Cross-checked against `'PageController page
//!   stability'` (line 360).
//! - `dragging_past_half_reads_the_next_page_dragging_below_half_reads_the_current_page`
//!   — the guarded `page()` getter tracks the LIVE drag position (no release
//!   needed — a pure function of the dragged `pixels`, so no gesture-timing
//!   noise). Cross-checked against `'Page changes at halfway point'` (line
//!   480), read through `controller.page()` instead of `onPageChanged`.
//! - `on_page_changed_fires_exactly_once_per_crossing` — the same halfway
//!   drag, read through the `on_page_changed` callback instead. Directly
//!   ports `'Page changes at halfway point'` (480)'s log-shape assertions
//!   (empty below half, `[1]` once crossed, empty again after further
//!   same-page movement).
//! - `a_full_drag_and_release_settles_the_page_view_through_the_real_gesture_and_spring_pipeline`
//!   — proves the full gesture → `PageScrollPhysics` → `ScrollSpringSimulation`
//!   → settle pipeline is wired end-to-end. Cross-checked against `'Bouncing
//!   scroll physics ballistics does not overshoot'` (531)'s "release and
//!   settle" shape (with `ClampingScrollPhysics` as this port's boundary
//!   physics, not `BouncingScrollPhysics`).
//! - `viewport_fraction_spacing_matches_the_configured_fraction` —
//!   `viewport_fraction: 0.8` end-to-end: a page's pixel offset is `page *
//!   viewport_dimension * 0.8`, not `* 1.0` — the geometric fact that makes
//!   neighboring pages peek into the viewport. Cross-checked against
//!   `'PageView viewportFraction'` (586) / `'PageView small viewportFraction'`
//!   (691).
//! - `controller_page_tracks_the_current_scroll_position` — `PageMetrics`
//!   (1064)'s "current page reflects `pixels`" contract.
//! - `jump_to_page_navigates_synchronously_without_animation` —
//!   `jumpToPage`'s "no animation, immediate" contract, exercised by
//!   `'PageController control test'` (309) and `'PageView viewportFraction'`
//!   (586).
//! - `resize_from_a_zero_size_viewport_does_not_lose_the_jumped_to_page` —
//!   the full collapse → jump-while-collapsed → restore pipeline (this is
//!   where `ScrollPosition::set_cached_page_while_collapsed` — PR2's fix for
//!   `PageController::jump_to_page`'s missing third branch — actually gets
//!   exercised end-to-end). Directly ports `'PageView resize from zero-size
//!   viewport should not lose state'` (64) and `'Change the page through the
//!   controller when zero-size viewport'` (110).
//!
//! ## Why "past half"/"below half"/"fling velocity" aren't ALSO gesture-driven
//! *release+settle* tests
//!
//! `PageScrollPhysics::create_ballistic_simulation`'s exact halfway threshold
//! and velocity-tolerance override are tested directly, at the physics-unit
//! level, in `crates/flui-widgets/src/scroll/page_view.rs`'s own `#[cfg(test)]`
//! module (`settles_forward_past_the_halfway_point_at_zero_velocity`,
//! `settles_backward_below_the_halfway_point_at_zero_velocity`,
//! `fling_velocity_beyond_tolerance_advances_regardless_of_distance`,
//! `backward_fling_velocity_beyond_tolerance_retreats_regardless_of_distance`)
//! — not duplicated here as gesture-driven release+settle assertions. Reason:
//! this crate's headless harness dispatches pointer events with real
//! `Instant::now()` timestamps advanced by only a minimal OS-timer tick
//! (`advance_gesture_clock`, `tests/common/mod.rs`), so a synthetic drag's
//! *measured* release velocity is enormous — `Scrollable::on_pan_end` clamps
//! it to Flutter's `kMaxFlingVelocity` (±8,000 px/s), still far above
//! `PageScrollPhysics::velocity_tolerance_px_per_sec` (20.0). A gesture-driven
//! settle test can't isolate "distance-only rounding" from "velocity-biased"
//! behavior deterministically that way — precisely the distinction those four
//! tests exist to pin with exact control over both inputs.
//! `dragging_past_half_reads_the_next_page_dragging_below_half_reads_the_current_page`
//! below still proves the halfway behavior end-to-end through a real drag —
//! it just reads the live (pre-release) position instead of a post-release
//! settle, which needs no velocity control at all. And
//! `a_full_drag_and_release_settles_the_page_view_through_the_real_gesture_and_spring_pipeline`
//! still proves gesture → physics → spring → settle wiring works, picked at a
//! distance where the outcome is the same whether or not a velocity bias
//! applies (see that test's own doc comment for the arithmetic).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::Vsync;
use flui_view::ViewExt;
use flui_widgets::{PageController, PageView, SizedBox, VsyncScope};

use crate::common::{lay_out, lay_out_with_arena, loose, tight};

/// Five identical, distinguishable-by-index-only pages — geometry/state
/// assertions read `PageController`, not page content, so the pages
/// themselves carry no identity.
fn pages(count: usize) -> Vec<flui_view::BoxedView> {
    (0..count)
        .map(|_| SizedBox::new(300.0, 300.0).boxed())
        .collect()
}

// ============================================================================
// initial_page
// ============================================================================

/// `PageController::with_params(2, 1.0)`'s `initial_page` must land on the
/// FIRST layout — the exact gap PR1's `DimensionChangePolicy` docs flagged as
/// deferred to "PR2's `PageController`", closed by wiring `initial_page:
/// Some(_)` into `DimensionChangePolicy::KeepFractionalPage` and
/// `ScrollPosition::apply_viewport_dimension`'s first-establishment branch.
///
/// Cross-checked against `'PageController control test'`
/// (`test/widgets/page_view_test.dart`, line 309): `PageController(initialPage:
/// 4)` shows `'California'` (index 4) on first pump, without any drag or
/// jump.
#[test]
fn initial_page_lands_on_first_layout() {
    let controller = PageController::with_params(2, 1.0);
    let widget = PageView::new(pages(5)).controller(controller.clone());

    let _laid = lay_out(widget, tight(300.0, 300.0));

    assert_eq!(
        controller.page(),
        Some(2.0),
        "initial_page (2) must be the current page immediately after the first layout"
    );
    assert_eq!(
        controller.scroll_controller().pixels(),
        600.0,
        "initial_page 2 at a 300px viewport, viewport_fraction 1.0, must land at \
         pixels = 2 * 300 * 1.0 = 600.0"
    );
}

// ============================================================================
// Resize
// ============================================================================

/// A `PageView` resize must preserve the current PAGE (not the raw pixel
/// offset) — `DimensionChangePolicy::KeepFractionalPage`'s steady-state
/// recompute (PR1), exercised end-to-end through a real `PageView`/
/// `Scrollable`/`Viewport` layout, not just the unit-level `ScrollPosition`
/// tests `crates/flui-rendering/src/view/scroll_position.rs` already pins.
///
/// Cross-checked against `'PageController page stability'`
/// (`test/widgets/page_view_test.dart`, line 360): a `PageView` resized
/// across several widths keeps showing the same page.
#[test]
fn resize_keeps_the_page_across_a_viewport_dimension_change() {
    let controller = PageController::with_params(2, 1.0);
    let page_view = PageView::new(pages(5)).controller(controller.clone());

    let mut laid = lay_out(
        SizedBox::new(300.0, 300.0).child(page_view.clone()),
        loose(1000.0),
    );
    assert_eq!(controller.page(), Some(2.0), "sanity: initial_page landed");
    assert_eq!(controller.scroll_controller().pixels(), 600.0);

    // Resize 300 -> 600: the PAGE (2.0) must be preserved, recomputing pixels
    // for the new dimension (2.0 * 600 * 1.0 = 1200.0) — not left at the
    // stale 600.0 pixel offset (which would show a different page).
    laid.pump_widget(SizedBox::new(600.0, 300.0).child(page_view.clone()));

    assert_eq!(
        controller.page(),
        Some(2.0),
        "a viewport resize must preserve the current PAGE, not the raw pixel offset"
    );
    assert_eq!(
        controller.scroll_controller().pixels(),
        1200.0,
        "the preserved page (2.0) must recompute against the new dimension \
         (2.0 * 600 * 1.0 = 1200.0)"
    );
}

// ============================================================================
// Halfway threshold — live drag position (no release/velocity involved)
// ============================================================================

/// The guarded `page()` getter tracks a LIVE drag's position — past the
/// halfway mark, it reads the next page; short of it, the current page. This
/// needs no gesture release or velocity control (a pure function of the
/// dragged `pixels`), so it isolates the halfway threshold cleanly — see the
/// module doc's "Why past half/below half/fling velocity aren't ALSO
/// gesture-driven release+settle tests" section for why the RELEASE-driven
/// spring behavior is instead tested at the physics-unit level.
///
/// Cross-checked against `'Page changes at halfway point'`
/// (`test/widgets/page_view_test.dart`, line 480): an 800px-wide viewport
/// crosses its halfway mark at -420px (`-380` short, `-420` past); this test
/// pins the same "more than half" threshold at a 300px page (half = 150px).
#[test]
fn dragging_past_half_reads_the_next_page_dragging_below_half_reads_the_current_page() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    scoped.dispatch_pointer_down(200.0, 100.0);
    scoped.dispatch_pointer_move(180.0, 100.0); // -20px: crosses slop, starts

    // -100px more (total 100px, page 0.333): short of half.
    scoped.dispatch_pointer_move(80.0, 100.0);
    assert_eq!(
        controller.page(),
        Some(100.0 / 300.0),
        "100px of 300 (page 0.333) is short of the halfway mark — page() must \
         still read the fractional CURRENT-side value, not have jumped to 1.0"
    );

    // -60px more (total 160px, page 0.533): past half.
    scoped.dispatch_pointer_move(20.0, 100.0);
    let page_after_crossing = controller
        .page()
        .expect("page() must be Some once laid out");
    assert!(
        (page_after_crossing - 160.0 / 300.0).abs() < 1e-4,
        "160px of 300 (page 0.533) is past the halfway mark; got {page_after_crossing}"
    );

    scoped.dispatch_pointer_up(20.0, 100.0);
}

/// Same halfway drag as above, read through `on_page_changed` instead of
/// `controller.page()` directly — the ROUNDED page, firing exactly once per
/// crossing.
///
/// Ports `'Page changes at halfway point'`
/// (`test/widgets/page_view_test.dart`, line 480)'s log-shape assertions
/// directly: empty while short of half, `[1]` once crossed, empty again for
/// further movement that stays on the same (now-current) page.
#[test]
fn on_page_changed_fires_exactly_once_per_crossing() {
    let log: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let log_cb = Arc::clone(&log);

    let controller = PageController::new();
    let widget = PageView::new(pages(5))
        .controller(controller.clone())
        .on_page_changed(move |page| {
            log_cb.lock().expect("not poisoned").push(page);
        });
    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    scoped.dispatch_pointer_down(200.0, 100.0);
    scoped.dispatch_pointer_move(180.0, 100.0); // -20px: crosses slop, starts

    // -100px more (total 100px, page 0.333): short of half — no fire.
    scoped.dispatch_pointer_move(80.0, 100.0);
    assert!(
        log.lock().expect("not poisoned").is_empty(),
        "short of the halfway mark must not fire on_page_changed"
    );

    // -60px more (total 160px, page 0.533): past half — fires once, page 1.
    scoped.dispatch_pointer_move(20.0, 100.0);
    assert_eq!(
        *log.lock().expect("not poisoned"),
        vec![1],
        "crossing the halfway mark must fire on_page_changed exactly once, with page 1"
    );

    // -20px more (total 180px, page 0.6): still rounds to page 1 — no
    // redundant re-fire.
    scoped.dispatch_pointer_move(0.0, 100.0);
    assert_eq!(
        *log.lock().expect("not poisoned"),
        vec![1],
        "further movement within the same rounded page must not re-fire on_page_changed"
    );

    scoped.dispatch_pointer_up(0.0, 100.0);
}

// ============================================================================
// Release + spring settle — the full pipeline, at an unambiguous distance
// ============================================================================

/// A genuine gesture → `PageScrollPhysics` → `ScrollSpringSimulation` →
/// settle proof, at a drag distance chosen so the outcome is IDENTICAL
/// whether or not this harness's inherently large release velocity (see the
/// module doc) applies its ±0.5 page bias: dragging to page 0.533 (past half)
/// settles to page 1 both at velocity 0 (`0.533` rounds to `1`) and under a
/// same-direction (`+0.5`) bias (`0.533 + 0.5 = 1.033`, still rounds to `1`).
/// This isolates "does the pipeline settle at all, to the right neighborhood"
/// — not the exact halfway threshold, which the physics-unit tests pin
/// precisely.
#[test]
fn a_full_drag_and_release_settles_the_page_view_through_the_real_gesture_and_spring_pipeline() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out_with_arena(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    scoped.dispatch_pointer_down(250.0, 100.0);
    scoped.dispatch_pointer_move(230.0, 100.0); // -20px: crosses slop, starts
    scoped.dispatch_pointer_move(70.0, 100.0); // -160px more (total 160px, page 0.533)
    scoped.dispatch_pointer_up(70.0, 100.0);

    // Pump enough frames for the spring (mass 0.5, stiffness 100.0, damping
    // ratio 1.1 — overdamped) to settle, mirroring
    // `bouncing_physics_top_overscroll_springs_back_to_min_extent`
    // (`tests/parity/scrollable_test.rs`).
    for _ in 0..100 {
        scoped.pump_for(Duration::from_millis(16));
    }

    let settled_pixels = controller.scroll_controller().pixels();
    assert!(
        (settled_pixels - 300.0).abs() < 1.0,
        "the full gesture -> physics -> spring pipeline must settle at page 1 \
         (pixels ~= 300.0) regardless of this harness's release-velocity bias \
         direction; got {settled_pixels:.2}"
    );
}

// ============================================================================
// viewport_fraction
// ============================================================================

/// `viewport_fraction: 0.8` end-to-end: a page's pixel spacing is `page *
/// viewport_dimension * 0.8`, not `* 1.0` — the geometric fact that a smaller
/// fraction leaves part of the viewport showing the neighboring page(s).
///
/// Cross-checked against `'PageView viewportFraction'`
/// (`test/widgets/page_view_test.dart`, line 586): a `viewportFraction`
/// smaller than 1.0 changes each page's pixel spacing by exactly that
/// fraction.
#[test]
fn viewport_fraction_spacing_matches_the_configured_fraction() {
    let controller = PageController::with_params(0, 0.8);
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let _laid = lay_out(widget, tight(300.0, 300.0));

    controller.jump_to_page(1);
    assert_eq!(
        controller.scroll_controller().pixels(),
        240.0,
        "at viewport_fraction 0.8 in a 300px viewport, page 1 must land at \
         1 * 300 * 0.8 = 240.0, not 300.0 (which viewport_fraction 1.0 would give) — \
         the 60px shortfall is exactly the neighboring page peeking into view"
    );

    controller.jump_to_page(2);
    assert_eq!(
        controller.scroll_controller().pixels(),
        480.0,
        "page 2 at viewport_fraction 0.8 must land at 2 * 300 * 0.8 = 480.0"
    );
}

// ============================================================================
// controller.page tracking + jump_to_page
// ============================================================================

/// `PageController::page` tracks the CURRENT scroll position as it changes —
/// not just what it read at layout time.
///
/// Cross-checked against `'PageMetrics'`
/// (`test/widgets/page_view_test.dart`, line 1064): `page` reflects the
/// scroll position's current `pixels`.
#[test]
fn controller_page_tracks_the_current_scroll_position() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let _laid = lay_out(widget, tight(300.0, 300.0));

    assert_eq!(controller.page(), Some(0.0), "starts at page 0");

    controller.position().set_pixels(450.0);
    assert_eq!(
        controller.page(),
        Some(1.5),
        "page() must track a direct pixel write (450 / 300 = 1.5), not a stale snapshot"
    );

    controller.position().set_pixels(900.0);
    assert_eq!(
        controller.page(),
        Some(3.0),
        "page() must track a second write (900 / 300 = 3.0)"
    );
}

/// `jump_to_page` navigates synchronously, without animation — the pixel
/// offset is exact and immediate, no frame pump needed.
///
/// Cross-checked against `'PageController control test'`
/// (`test/widgets/page_view_test.dart`, line 309) and `'PageView
/// viewportFraction'` (586), both of which read the jumped-to page
/// immediately after `jumpToPage`/a single `pump`.
#[test]
fn jump_to_page_navigates_synchronously_without_animation() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let _laid = lay_out(widget, tight(300.0, 300.0));

    controller.jump_to_page(3);

    assert_eq!(
        controller.scroll_controller().pixels(),
        900.0,
        "jump_to_page(3) must land at 3 * 300 * 1.0 = 900.0 immediately, with no animation"
    );
    assert_eq!(controller.page(), Some(3.0));
}

// ============================================================================
// Zero-size viewport resilience
// ============================================================================

/// The full collapse -> jump-while-collapsed -> restore pipeline: a
/// `PageController` navigated while its `PageView` sits inside a
/// currently-zero-size ancestor must not lose that navigation once the
/// ancestor resizes back to a real size. Two fixes made this pass together:
///
/// - `ScrollPosition::apply_viewport_dimension`'s short-circuit used to
///   compare raw `f32` values, so a first-ever call carrying literally `0.0`
///   (a `PageView` mounted inside an already-zero-size ancestor) matched
///   `State::zero()`'s own default and silently no-opped — `has_
///   applied_viewport_dimension` never flipped true, unlike Flutter's
///   `hasViewportDimension => _viewportDimension != null`, whose null
///   comparison never short-circuits a genuinely first call. Fixed by gating
///   the short-circuit on `has_applied_viewport_dimension` itself.
/// - `ScrollPosition::set_cached_page_while_collapsed` — the fix for
///   `PageController::jump_to_page`'s missing third branch (Flutter's
///   `_cachedPage != null` check). Without it, `jump_to_page` while collapsed
///   computes pixels from the CURRENT (zero) dimension (landing at `0.0`)
///   instead of overwriting the cached page, so the restore below would
///   recompute page 0, not 4.
///
/// Directly ports `'PageView resize from zero-size viewport should not lose
/// state'` (`test/widgets/page_view_test.dart`, line 64) and `'Change the
/// page through the controller when zero-size viewport'` (110).
#[test]
fn resize_from_a_zero_size_viewport_does_not_lose_the_jumped_to_page() {
    let controller = PageController::new();
    let page_view = PageView::new(pages(5)).controller(controller.clone());

    let mut laid = lay_out(
        SizedBox::new(0.0, 0.0).child(page_view.clone()),
        loose(1000.0),
    );
    assert_eq!(
        controller.scroll_controller().viewport_dimension_pixels(),
        0.0,
        "sanity: the viewport is genuinely collapsed to zero after the first layout"
    );

    // Navigate while collapsed — must take effect once the viewport resizes,
    // not be silently lost or misread as pixels = 0.
    controller.jump_to_page(4);

    laid.pump_widget(SizedBox::new(300.0, 300.0).child(page_view.clone()));

    assert_eq!(
        controller.page(),
        Some(4.0),
        "a page jump requested while the viewport was collapsed must be honored \
         once it resizes to a real dimension"
    );
    assert_eq!(
        controller.scroll_controller().pixels(),
        1200.0,
        "the jumped-to page (4) must recompute against the restored dimension \
         (4 * 300 * 1.0 = 1200.0), not read as page 0 (which the collapsed \
         pixels value of 0.0 would wrongly suggest)"
    );
}
