//! Flutter parity tests — `PageView`.
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
//! `PageStorage`/`viewport_fraction > 1.0` centering). **15
//! tests ported below**, covering the portable core:
//!
//! - `initial_page_lands_on_first_layout` — `initialPage` seeding on the very
//!   first layout. Cross-checked against `'PageController control test'`
//!   (line 309).
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
//!   the full collapse → jump-while-collapsed → restore pipeline, including
//!   `controller.page()` reading the pending page WHILE still collapsed
//!   (`ScrollPosition::cached_page`). Directly ports `'PageView resize from
//!   zero-size viewport should not lose state'` (64) and `'Change the page
//!   through the controller when zero-size viewport'` (110).
//! - `a_rebuild_that_swaps_the_on_page_changed_callback_fires_the_new_one` —
//!   a rebuild that only changes `on_page_changed` (same controller) must
//!   fire the NEW callback, not whatever `init_state` first saw.
//! - `swapping_the_controller_then_disposing_does_not_leak_or_collide_across_notifiers`
//!   — a controller swap unsubscribes from the OLD controller's notifier
//!   specifically and resubscribes to the new one; a later dispose removes
//!   only this widget's own listener, not a pre-existing, unrelated listener
//!   sharing the new controller's notifier (a per-notifier `ListenerId`
//!   counter makes a naive "remove by id from whatever the current
//!   controller resolves to" collide).
//! - `a_parent_rebuild_with_no_explicit_controller_keeps_the_current_page` —
//!   `PageView` without an explicit `.controller(...)` owns a default
//!   `PageController` inside its own state, created once and kept alive
//!   across every rebuild that doesn't pass an explicit one; a parent
//!   rebuild must not reset the page to `0`.
//! - `vertical_page_view_drag_crosses_to_the_next_page` — `Axis::Vertical`
//!   coverage (the horizontal axis dominates the rest of this file, but
//!   `PageView::scroll_direction` is not axis-agnostic-by-construction — a
//!   `dy`-vs-`dx` mixup would only show up in a real vertical drag).
//! - `animate_to_page_lands_on_the_page` (ADR-0037) — page → pixels
//!   through the guarded formula, then a real curve/duration animation
//!   pumped to completion. Cross-checked against `'PageController control
//!   test'` (309), which exercises `animateToPage` the same way this port's
//!   `PageController::animate_to_page` does.
//! - `next_page_and_previous_page_navigate_and_clamp_at_the_ends`
//!   (ADR-0037) — an ordinary one-page step, then `next_page` past the
//!   last real page and `previous_page` below the first: both clamp at
//!   `max_scroll_extent`/`0.0` via `ScrollController::animate_to`'s own
//!   clamp, a documented divergence from the oracle's physics-clamped ticks
//!   (see `PageController::next_page`'s doc).
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

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::{Curves, Vsync};
use flui_types::layout::Axis;
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, IntoView, ViewExt};
use flui_widgets::{GestureArenaScope, PageController, PageView, SizedBox, VsyncScope};

use crate::common::{lay_out, lay_out_with_arena, loose, tight};

/// Five identical, distinguishable-by-index-only pages — geometry/state
/// assertions read `PageController`, not page content, so the pages
/// themselves carry no identity.
fn pages(count: usize) -> Vec<flui_view::BoxedView> {
    (0..count)
        .map(|_| SizedBox::new(300.0, 300.0).boxed())
        .collect()
}

/// Drags the pointer horizontally by `total_delta_x` px (negative = finger
/// moves left, INCREASING `pixels` per `Scrollable`'s convention), split into
/// multiple bounded down/move/move/up sessions so every dispatched
/// coordinate stays on-canvas within a 300px-wide viewport.
///
/// `LaidOut::route_event` re-hit-tests at the exact `(x, y)` of every
/// dispatched event, including moves (`tests/common/mod.rs`) — a single
/// session covering more distance than the viewport is wide would dispatch
/// an off-canvas move (e.g. a negative x) that silently fails to hit-test
/// and never reaches the recognizer. Splitting into sessions (each a
/// separate down/up cycle, like a user re-gripping mid-scroll) keeps every
/// coordinate within `[50, 250]` while still accumulating onto the SAME
/// underlying `ScrollPosition` across sessions.
fn drag_horizontal_by(scoped: &crate::common::LaidOutScoped, total_delta_x: f32, y: f32) {
    const START_X: f32 = 150.0;
    const MAX_CHUNK: f32 = 100.0;
    const SLOP: f32 = 20.0;

    let mut remaining = total_delta_x;
    while remaining.abs() > f32::EPSILON {
        let chunk = remaining.clamp(-MAX_CHUNK, MAX_CHUNK);
        let slop_offset = if chunk < 0.0 { -SLOP } else { SLOP };
        // The slop-crossing move reports no delta (matches the established
        // convention throughout this file); the SECOND move's delta from
        // THAT position is what `on_pan_update` sees — so the final position
        // must be `slop_offset + chunk` past `START_X`, not just `chunk`
        // past it, for this session to contribute exactly `chunk`.
        let final_x = START_X + slop_offset + chunk;
        scoped.dispatch_pointer_down(START_X, y);
        scoped.dispatch_pointer_move(START_X + slop_offset, y); // crosses slop, starts
        scoped.dispatch_pointer_move(final_x, y); // fires on_pan_update with delta == chunk
        scoped.dispatch_pointer_up(final_x, y);
        remaining -= chunk;
    }
}

// ============================================================================
// initial_page
// ============================================================================

/// `PageController::with_params(2, 1.0)`'s `initial_page` must land on the
/// FIRST layout, wired through `initial_page: Some(_)` on
/// `DimensionChangePolicy::KeepFractionalPage` and
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
/// recompute, exercised end-to-end through a real `PageView`/`Scrollable`/
/// `Viewport` layout, not just the unit-level `ScrollPosition` tests
/// `crates/flui-rendering/src/view/scroll_position.rs` already pins.
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

    // The oracle asserts `controller.page == kStates.indexOf('Iowa')`
    // (`'Change the page through the controller when zero-size viewport'`,
    // line 110) WHILE STILL zero-size, before any resize — `page()` must
    // read the pending page from `ScrollPosition::cached_page`, not
    // `pixels / 0`-collapsed `0.0`.
    assert_eq!(
        controller.page(),
        Some(4.0),
        "page() must read the jumped-to page from the cache while still \
         collapsed, not report page 0"
    );

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

// ============================================================================
// Rebuild-time callback / controller updates
// ============================================================================

/// A rebuild that swaps ONLY `on_page_changed` (the controller stays the
/// same) must fire the NEW callback, not the one `init_state` first saw.
/// `PageViewState`'s listener dereferences a shared, mutable slot at CALL
/// time — `did_update_view` writes through it on every rebuild — rather than
/// closing over a one-time snapshot of the callback.
#[test]
fn a_rebuild_that_swaps_the_on_page_changed_callback_fires_the_new_one() {
    let old_log: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let new_log: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let controller = PageController::new();

    let old_log_cb = Arc::clone(&old_log);
    let widget = PageView::new(pages(5))
        .controller(controller.clone())
        .on_page_changed(move |page| {
            old_log_cb.lock().expect("not poisoned").push(page);
        });
    let mut laid = lay_out(widget, tight(300.0, 300.0));

    // Rebuild with the SAME controller, a DIFFERENT on_page_changed closure.
    let new_log_cb = Arc::clone(&new_log);
    laid.pump_widget(
        PageView::new(pages(5))
            .controller(controller.clone())
            .on_page_changed(move |page| {
                new_log_cb.lock().expect("not poisoned").push(page);
            }),
    );

    controller.jump_to_page(2);

    assert_eq!(
        *new_log.lock().expect("not poisoned"),
        vec![2],
        "the callback installed by the rebuild must fire for a page change \
         that happens AFTER the rebuild"
    );
    assert!(
        old_log.lock().expect("not poisoned").is_empty(),
        "the callback that was swapped OUT by the rebuild must never fire again"
    );
}

/// A `StatelessView` host that can unmount `PageView` entirely (`show:
/// false`) or swap which `PageController` it hands to a mounted one — the
/// same "stable root TYPE, varying build output" pattern
/// `InteractiveViewerHost` (`tests/parity/interactive_viewer_test.rs`) uses,
/// since `pump_widget` reconciling two DIFFERENT concrete root types does not
/// run the normal unmount/dispose path.
#[derive(Clone, StatelessView)]
struct PageViewHost {
    controller: PageController,
    show: bool,
}

impl StatelessView for PageViewHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if !self.show {
            return SizedBox::new(1.0, 1.0).into_view().boxed();
        }
        PageView::new(pages(5))
            .controller(self.controller.clone())
            .into_view()
            .boxed()
    }
}

/// Swapping `PageView`'s controller must unsubscribe from the OLD
/// controller's notifier specifically — not whatever `self.controller`
/// resolves to AFTER the swap — and a later dispose must remove only THIS
/// widget's own listener(s), leaving a pre-existing, unrelated listener
/// sharing the new controller's notifier untouched. A per-notifier
/// `ListenerId` counter means a naive "always remove from `self.controller`'s
/// current listenable" can collide: the pre-existing listener below is
/// deliberately registered on `new_controller` BEFORE `PageView` ever adopts
/// it, so it gets the FIRST id `new_controller`'s (fresh) counter issues —
/// the same slot one of `PageView`'s own listeners would land in in a naive
/// implementation that resolved the listenable fresh from `self.controller`
/// at removal time instead of storing it at registration time.
#[test]
fn swapping_the_controller_then_disposing_does_not_leak_or_collide_across_notifiers() {
    // A mounted `PageView` puts TWO listeners on its controller's notifier:
    // `Scrollable`'s own `AnimatedBuilder` subscription (`scroll_controller
    // .as_listenable()` inside `PageViewState::build`, sharing the exact same
    // underlying `ScrollPosition` `PageController::as_listenable()` does) and
    // `PageViewState`'s own `on_page_changed`-tracking listener.
    const PAGE_VIEW_LISTENER_COUNT: usize = 2;

    let old_controller = PageController::new();
    let new_controller = PageController::new();

    let external_notified = Arc::new(AtomicUsize::new(0));
    let external_notified_cb = Arc::clone(&external_notified);
    let _external_listener_id = new_controller
        .as_listenable()
        .add_listener(Arc::new(move || {
            external_notified_cb.fetch_add(1, Ordering::SeqCst);
        }));

    let mut laid = lay_out(
        PageViewHost {
            controller: old_controller.clone(),
            show: true,
        },
        tight(300.0, 300.0),
    );
    assert_eq!(
        old_controller.position().len(),
        PAGE_VIEW_LISTENER_COUNT,
        "sanity: mounting subscribes Scrollable + PageView's own listener to old_controller"
    );
    assert_eq!(
        new_controller.position().len(),
        1,
        "sanity: only the pre-existing external listener is on new_controller so far"
    );

    // Swap controllers on a rebuild — still mounted.
    laid.pump_widget(PageViewHost {
        controller: new_controller.clone(),
        show: true,
    });

    assert_eq!(
        old_controller.position().len(),
        0,
        "the old controller's listeners must be removed after the swap, not leaked"
    );
    assert_eq!(
        new_controller.position().len(),
        1 + PAGE_VIEW_LISTENER_COUNT,
        "the new controller must gain Scrollable's + PageView's listeners \
         alongside the pre-existing external one"
    );

    // Unmount entirely.
    laid.pump_widget(PageViewHost {
        controller: new_controller.clone(),
        show: false,
    });

    assert_eq!(
        new_controller.position().len(),
        1,
        "disposing must remove ONLY PageView's (and Scrollable's) own \
         listeners from new_controller, leaving the pre-existing external one \
         intact — a collision would either remove the external listener too \
         (count 0) or fail to remove PageView's own (count stays higher)"
    );

    new_controller.position().set_pixels(123.0);
    assert_eq!(
        external_notified.load(Ordering::SeqCst),
        1,
        "the surviving external listener must still actually fire after PageView's dispose"
    );
}

/// `PageView` with no explicit `.controller(...)` owns a default
/// `PageController` inside its own state — created once, kept alive across
/// every rebuild that doesn't pass an explicit one. A parent rebuild handing
/// a FRESH `PageView` value (still no controller) must not discard that
/// state-owned controller (and the page it's currently tracking) in favor of
/// a brand-new one starting back at page 0.
///
/// The root stays the SAME `PageView` concrete type across the rebuild (a
/// `pump_widget` reconciliation, not a remount), so `PageViewState` persists
/// — this test proves `did_update_view` doesn't unconditionally overwrite
/// that persisted state's controller from `new_view.controller` on every
/// build (the bug: `PageView::new` used to always construct a FRESH default
/// controller, silently discarding the current page on every rebuild).
///
/// Distinguishing "preserved" from "reset" needs an actual PROBE after the
/// rebuild, not just an absence of a spurious `on_page_changed` fire (a
/// silent controller-field reset alone doesn't itself fire anything — no
/// gesture moves the position when the rebuild happens). This drags to an
/// unambiguous page (pixels 300.0, page 1), rebuilds with no controller (still
/// wrapped in the SAME outer `GestureArenaScope<PageView>` root type, so this
/// is a genuine `pump_widget` update — see `PageViewHost`'s doc for why a bare
/// root type swap wouldn't be), then drags 650px further via
/// [`drag_horizontal_by`]'s bounded chunks — which, since `on_page_changed`
/// fires at EVERY rounded-page crossing (not just the final one, see
/// `on_page_changed_fires_exactly_once_per_crossing`), reports every page it
/// passes through, not just where it lands: preserved (pixels 300 -> 950
/// across `+100, +100, +100, +100, +100, +100, +50` chunks) crosses into
/// rounds-to-2 territory once, then rounds-to-3 once, reporting `[2, 3]`;
/// reset-to-a-fresh-controller (pixels 0 -> 650 via the same chunks) instead
/// crosses `0 -> 1 -> 2`, but starts by DROPPING back through round-to-0
/// first (`last_reported_page` itself survives the rebuild either way — it
/// is not part of the controller being reset — so from its post-phase-1
/// value of `1`, landing back near pixels 0 registers as a crossing back to
/// `0` before climbing again), reporting `[0, 1, 2]`. The two sequences share
/// no page in common, so this is unambiguous either way.
#[test]
fn a_parent_rebuild_with_no_explicit_controller_keeps_the_current_page() {
    let log: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));

    let log_cb = Arc::clone(&log);
    let widget = PageView::new(pages(5)).on_page_changed(move |page| {
        log_cb.lock().expect("not poisoned").push(page);
    });
    let mut scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    // Phase 1: drag to pixels = 300.0 exactly (page 1.0).
    drag_horizontal_by(&scoped, -300.0, 100.0);
    assert_eq!(
        *log.lock().expect("not poisoned"),
        vec![1],
        "sanity: the first drag reached page 1"
    );

    // Parent rebuild with NO explicit controller: a fresh `PageView` value,
    // re-wrapped in a matching `GestureArenaScope` so the root's own
    // concrete type stays identical across the swap (`PageViewHost`'s doc
    // explains why a bare type change wouldn't reconcile as an update).
    let log_cb2 = Arc::clone(&log);
    let rewrapped = GestureArenaScope::new(
        scoped.laid().arena(),
        PageView::new(pages(5)).on_page_changed(move |page| {
            log_cb2.lock().expect("not poisoned").push(page);
        }),
    );
    scoped.pump_widget(rewrapped);

    // Phase 2: drag 650px further, in bounded chunks (see
    // `drag_horizontal_by`'s docs) — which cross MULTIPLE page boundaries
    // along the way, each firing its own `on_page_changed`.
    drag_horizontal_by(&scoped, -650.0, 100.0);

    assert_eq!(
        *log.lock().expect("not poisoned"),
        vec![1, 2, 3],
        "the rebuild must have kept the page-1 position (pixels 300.0) alive: \
         a further 650px drag must climb straight through pages 2 and 3 \
         (950 / 300 = 3.167 final), not first drop back to page 0 on the way \
         up from a reset pixels-0.0 start (0 -> 1 -> 2, which discarding the \
         state-owned controller for a fresh one would produce)"
    );
}

// ============================================================================
// Vertical axis
// ============================================================================

/// `Axis::Vertical` coverage: a vertical `PageView` must track `dy` drags the
/// same way the rest of this file's horizontal cases track `dx` — the only
/// test in this file that isn't `Axis::Horizontal` (the module default),
/// closing the gap a `dy`-vs-`dx` mixup in `Scrollable`'s axis handling could
/// otherwise hide.
#[test]
fn vertical_page_view_drag_crosses_to_the_next_page() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5))
        .controller(controller.clone())
        .scroll_direction(Axis::Vertical);
    let scoped = lay_out_with_arena(widget, tight(300.0, 300.0));

    scoped.dispatch_pointer_down(100.0, 200.0);
    scoped.dispatch_pointer_move(100.0, 180.0); // -20px dy: crosses slop, starts
    scoped.dispatch_pointer_move(100.0, 20.0); // -160px dy more: pixels = 160.0

    let page = controller
        .page()
        .expect("page() must be Some once laid out");
    assert!(
        (page - 160.0 / 300.0).abs() < 1e-4,
        "a vertical PageView must track dy drags exactly like a horizontal one \
         tracks dx; expected page {:.3}, got {page:.3}",
        160.0 / 300.0
    );

    scoped.dispatch_pointer_up(100.0, 20.0);
}

// ============================================================================
// animate_to_page / next_page / previous_page (ADR-0037)
// ============================================================================

/// `animate_to_page` navigates via a real curve/duration animation — page →
/// pixels through the guarded `ScrollMetrics::pixels_from_page` formula, then
/// delegates to `ScrollController::animate_to`. Pumping past the duration
/// must land EXACTLY on the target page's pixel offset.
///
/// Cross-checked against `'PageController control test'`
/// (`test/widgets/page_view_test.dart`, line 309), which drives
/// `animateToPage` the same way this port's `PageController::animate_to_page`
/// does.
#[test]
fn animate_to_page_lands_on_the_page() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out_with_arena(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    controller.animate_to_page(3, Duration::from_millis(100), Arc::new(Curves::Linear));

    // Comfortably past the 100ms duration (see `scroll.rs`'s
    // `scrollable_animate_to_reaches_the_target_through_the_curve` for why
    // driving through the queued-command path needs a few warm-up pumps).
    for _ in 0..20 {
        scoped.pump_for(Duration::from_millis(16));
    }

    assert_eq!(
        controller.scroll_controller().pixels(),
        900.0,
        "animate_to_page(3) must land at 3 * 300 * 1.0 = 900.0 once the \
         animation has fully elapsed"
    );
    assert_eq!(controller.page(), Some(3.0));
}

/// `next_page`/`previous_page` step by one whole page from the current
/// (rounded) page. Past the last real page or below the first, the run
/// clamps at `max_scroll_extent`/`0.0` — via `ScrollController::animate_to`'s
/// own clamp, not a page-count check `PageController` has no way to make
/// (matching the oracle: `nextPage`/`previousPage` are plain
/// `animateToPage(page!.round() +/- 1, ...)` calls with no bounds check of
/// their own — see `PageController::next_page`'s doc for how FLUI reaches the
/// same visible stopping point through a different mechanism than the
/// oracle's boundary-clamped per-tick `setPixels`).
#[test]
fn next_page_and_previous_page_navigate_and_clamp_at_the_ends() {
    let controller = PageController::new();
    let widget = PageView::new(pages(5)).controller(controller.clone());
    let vsync = Vsync::new();
    let wrapped = VsyncScope::new(vsync.clone(), widget);
    let mut scoped = lay_out_with_arena(wrapped, tight(300.0, 300.0));
    scoped.adopt_vsync(vsync);

    // Ordinary step: page 0 -> 1.
    controller.next_page(Duration::from_millis(50), Arc::new(Curves::Linear));
    for _ in 0..20 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.scroll_controller().pixels(),
        300.0,
        "next_page from page 0 must land on page 1 (300.0)"
    );

    // Jump to the last real page (4 of 5 — index 4), then next_page must
    // clamp AT it, never overshooting to a nonexistent page 5.
    controller.jump_to_page(4);
    controller.next_page(Duration::from_millis(50), Arc::new(Curves::Linear));
    for _ in 0..20 {
        scoped.pump_for(Duration::from_millis(16));
    }
    let max_scroll_extent = controller.scroll_controller().max_scroll_extent();
    assert_eq!(
        controller.scroll_controller().pixels(),
        max_scroll_extent,
        "next_page past the last real page must clamp at max_scroll_extent \
         ({max_scroll_extent}), not overshoot toward a page-5 pixel offset"
    );

    // Jump to the first page, then previous_page must clamp at 0, never
    // going negative.
    controller.jump_to_page(0);
    controller.previous_page(Duration::from_millis(50), Arc::new(Curves::Linear));
    for _ in 0..20 {
        scoped.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        controller.scroll_controller().pixels(),
        0.0,
        "previous_page below the first page must clamp at 0.0, not go negative"
    );
}
