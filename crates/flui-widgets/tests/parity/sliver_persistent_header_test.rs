//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver_persistent_header.dart`
//!   `SliverPersistentHeader`.
//! - Render objects: `packages/flutter/lib/src/rendering/sliver_persistent_header.dart`
//!   (the pinned × floating matrix).
//! - Tests: `packages/flutter/test/widgets/sliver_persistent_header_test.dart`
//!   (22 `testWidgets` cases).
//!
//! Widget → render-object mapping: FLUI `SliverPersistentHeader` (facade in
//! `flui-widgets`, element in `flui-view`, four render variants in
//! `flui-objects`); the delegate child is built through the
//! build-during-layout seam, so it appears one fixpoint pass after layout
//! publishes — same frame, matching the oracle's synchronous `build`.
//!
//! # Case ledger (all 22 accounted; `sliver_test_utils.dart` fixtures below)
//!
//! | oracle line | case | status |
//! |---|---|---|
//! | :36 | scrolling updates stretchConfiguration | PORTED |
//! | :70 | pinned updates stretchConfiguration | PORTED |
//! | :105 | pinned updates showOnScreenConfiguration | DEFERRED: `show_on_screen` is omitted at the render layer by documented design (`sliver_persistent_header.rs` module doc) — there is no configuration to update |
//! | :140 | floating — scroll offset doesn't change | PORTED |
//! | :173 | floating — normal behavior | PORTED |
//! | :301 | floating — no floating during a position animation | PORTED (`ScrollController::animate_to` drives a real curve run through the fling controller; the run raises the scroll-activity signal but records no USER direction, which is exactly what keeps the float suppressed) |
//! | :346 | floating — behavior when dragging | PORTED (no gestures needed after all: the oracle forces `updateUserScrollDirection(forward)` around an `animateTo` — FLUI's `set_user_scroll_direction`, called once the run's activity is live) |
//! | :400 | floating — overscroll gap below header | PORTED |
//! | :437 | pinned | PORTED |
//! | :484 | toStringDeep of throwing maxExtent | NOT PORTABLE: FLUI delegate extents are infallible `f32` getters — the throwing-getter diagnostic path has no analogue |
//! | :552 | pinned with slow scroll | PORTED (nine settled `animateTo` steps, all offsets/visibility/child-box rects literal) |
//! | :673 | pinned with less overlap | PORTED |
//! | :720 | overscroll gap is below header | PORTED (the :883 scrolling variant is the same assertion set; both ported as one) |
//! | :759 | pointer scrolled floating | PORTED (wheel dispatch exists now; geometry asserts carry the case — the oracle's `find.text` presence probes are residency checks, and FLUI's eager `SliverFixedExtentList` keeps every item mounted, which makes text presence vacuous here — the established `find_text` ruling from the SliverFixedExtentList port) |
//! | :812 | scrolling | PORTED |
//! | :851 | scrolling off screen | PORTED |
//! | :883 | scrolling — overscroll gap below header | PORTED (see :720) |
//! | :910 | (unnamed) pinned+floating with bottom — opacity fade | DEFERRED: asserts the material `SliverAppBar` toolbar-opacity fade, covered value-exact by `flui-material`'s `toolbar_opacity_follows_the_oracle_branches` unit tests |
//! | :945 | semantics — within viewport | DEFERRED: `find.semantics`-family case; FLUI's semantics query harness lives in `flui-testing` (`a11y_tree`), not this crate's parity harness — porting rides a harness bridge, not header work |
//! | :974 | semantics — partially scrolling off screen | DEFERRED: same |
//! | :1015 | semantics — completely off screen within cache extent | DEFERRED: same |
//! | :1052 | semantics — completely off screen, not within cache extent | DEFERRED: same |
//!
//! # Content sweep (standing rule)
//!
//! `grep -l "SliverPersistentHeader" .flutter/packages/flutter/test/` also
//! hits `material/sliver_app_bar_test.dart` (subject: `SliverAppBar`),
//! `widgets/slivers_evil_test.dart` (subject: sliver-removal robustness),
//! and `widgets/nested_scroll_view_test.dart` (subject: `NestedScrollView`)
//! — all use the header as scene scaffolding for a different subject;
//! incidental, 0 subject cases beyond the 22 above.
//!
//! # Fixture equivalences
//!
//! - `TestDelegate` (min = max = 200, child labelled 200-high box) →
//!   [`fixed_delegate`].
//! - `TestDelegate2` (min 100, max 200) → [`ShrinkingDelegate`]. Its child
//!   mirrors the oracle's childless `Container(constraints:
//!   BoxConstraints(minHeight: 100, maxHeight: 200))`, which EXPANDS to the
//!   enforced max — the box-rect cases read that child's committed rect, so
//!   a fixed-height stand-in would distort them.
//! - `BigSliver(height)` (a plain fixed-extent sliver) →
//!   `SliverToBoxAdapter` over a `SizedBox` of that height.
//! - `verifyPaintPosition(key, offset, visible)` → the harness's sliver
//!   `offset(render_id)` (the committed `SliverPhysicalParentData` paint
//!   offset) plus `sliver_geometry(render_id).visible`.
//! - `position.animateTo(x) + pumpAndSettle` → `controller.jump_to(x)` +
//!   `pump()`: the oracle's linear minute-long animation exists only to
//!   arrive at `x` settled, and FLUI's controller commits the same
//!   destination synchronously.
//! - The oracle viewport is 800×600; every literal below keeps those
//!   dimensions so the expected offsets read identically.

use std::cell::RefCell;
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_rendering::view::ScrollDirection;
use flui_view::BoxedView;
use flui_view::IntoView;
use flui_view::view::ViewExt;
use flui_widgets::{
    ConstrainedBox, OverScrollHeaderStretchConfiguration, ScrollController, Scrollable, SizedBox,
    SliverFixedExtentList, SliverPersistentHeader, SliverPersistentHeaderDelegate,
    SliverToBoxAdapter, Viewport,
};

use crate::common::{LaidOut, lay_out, tight};
use flui_animation::{Curves, Vsync};
use flui_widgets::VsyncScope;
use std::sync::Arc;
use std::time::Duration;

/// `TestDelegate`: min = max = 200, fixed 200-high child.
struct FixedDelegate {
    stretch: Option<OverScrollHeaderStretchConfiguration>,
    builds: Rc<RefCell<Vec<(f32, bool)>>>,
}

fn fixed_delegate() -> FixedDelegate {
    FixedDelegate {
        stretch: None,
        builds: Rc::new(RefCell::new(Vec::new())),
    }
}

impl SliverPersistentHeaderDelegate for FixedDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        shrink_offset: f32,
        overlaps_content: bool,
    ) -> BoxedView {
        self.builds
            .borrow_mut()
            .push((shrink_offset, overlaps_content));
        SizedBox::new(800.0, 200.0).into_view().boxed()
    }

    fn min_extent(&self) -> f32 {
        200.0
    }

    fn max_extent(&self) -> f32 {
        200.0
    }

    fn stretch_configuration(&self) -> Option<OverScrollHeaderStretchConfiguration> {
        self.stretch.clone()
    }
}

/// `TestDelegate2`: min 100, max 200 — and, faithfully, a child that
/// FLEXES with the header (`Container(constraints: BoxConstraints(
/// minHeight: minExtent, maxHeight: maxExtent))`), because the
/// `verifyActualBoxPosition` cases assert the CHILD box's committed rect,
/// which a fixed-height child would distort.
struct ShrinkingDelegate;

impl SliverPersistentHeaderDelegate for ShrinkingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        _shrink_offset: f32,
        _overlaps_content: bool,
    ) -> BoxedView {
        ConstrainedBox::new(flui_rendering::constraints::BoxConstraints {
            min_width: flui_types::geometry::px(0.0),
            max_width: flui_types::geometry::px(f32::INFINITY),
            min_height: flui_types::geometry::px(100.0),
            max_height: flui_types::geometry::px(200.0),
        })
        .child(SizedBox::expand())
        .into_view()
        .boxed()
    }

    fn min_extent(&self) -> f32 {
        100.0
    }

    fn max_extent(&self) -> f32 {
        200.0
    }
}

/// `BigSliver(height)` — a plain box sliver of the given height.
fn big_sliver(height: f32) -> BoxedView {
    SliverToBoxAdapter::new()
        .child(SizedBox::new(800.0, height))
        .into_view()
        .boxed()
}

/// Mount `slivers` in an 800×600 position-mode viewport driven by
/// `controller` — the oracle's `CustomScrollView` + `ScrollableState`
/// position, minus gestures (every ported case scrolls programmatically).
fn mount(controller: &ScrollController, slivers: Vec<BoxedView>) -> LaidOut {
    let position = controller.position();
    lay_out(
        Scrollable::new()
            .controller(controller.clone())
            .viewport_builder(Rc::new(move |_| {
                Viewport::new(slivers.clone())
                    .position(position.clone())
                    .boxed()
            })),
        tight(800.0, 600.0),
    )
}

/// `verifyPaintPosition` — the committed sliver paint offset and, when
/// given, the sliver geometry's visibility.
fn verify_paint_position(laid: &LaidOut, id: RenderId, ideal: (f32, f32), visible: Option<bool>) {
    let offset = laid.offset(id);
    assert_eq!(
        (offset.dx.get(), offset.dy.get()),
        ideal,
        "sliver paint offset diverges from the oracle"
    );
    if let Some(visible) = visible {
        assert_eq!(
            laid.sliver_geometry(id).visible,
            visible,
            "sliver visibility diverges from the oracle"
        );
    }
}

/// `verifyActualBoxPosition` — the committed absolute rect (root-local
/// offset + size) of a header's child BOX, against the oracle's literal.
fn verify_actual_box_position(laid: &LaidOut, box_id: RenderId, ideal: (f32, f32, f32, f32)) {
    let offset = laid.absolute_offset(box_id);
    let size = laid.size(box_id);
    assert_eq!(
        (
            offset.dx.get(),
            offset.dy.get(),
            size.width.get(),
            size.height.get()
        ),
        ideal,
        "the header child box's committed rect diverges from the oracle"
    );
}

/// The N-th sliver child of the viewport, in paint order.
fn viewport_child(laid: &LaidOut, index: usize) -> RenderId {
    let viewport = laid.find_by_render_type("RenderViewport");
    laid.child(viewport, index)
}

// ─────────────────────────────────────────────────────────────────────────────
// :36 / :70 — stretchConfiguration updates reach the live render object
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: pump twice with trigger offsets 10 then 20; the LIVE render
/// object must carry the second value. Pins that a delegate swap re-pushes
/// the stretch configuration through `update_render_object` instead of
/// freezing the mount-time value. One test per variant, as in the oracle.
fn stretch_configuration_updates(pinned: bool, render_type: &str) {
    let controller = ScrollController::new();
    let build = move |trigger: f32| -> Vec<BoxedView> {
        vec![
            SliverPersistentHeader::new(FixedDelegate {
                stretch: Some(OverScrollHeaderStretchConfiguration::new(trigger, None)),
                builds: Rc::new(RefCell::new(Vec::new())),
            })
            .pinned(pinned)
            .into_view()
            .boxed(),
            big_sliver(300.0),
        ]
    };

    let mut laid = mount(&controller, build(10.0));
    let position = controller.position();
    laid.pump_widget(
        Scrollable::new()
            .controller(controller.clone())
            .viewport_builder(Rc::new(move |_| {
                Viewport::new(build(20.0))
                    .position(position.clone())
                    .boxed()
            })),
    );

    let header = laid.find_by_render_type(render_type);
    assert_eq!(
        laid.render_property(header, "stretch_trigger_offset")
            .as_deref(),
        Some("20"),
        "the SECOND pump's stretch trigger must reach the live render object"
    );
}

#[test]
fn scrolling_header_updates_stretch_configuration() {
    stretch_configuration_updates(false, "RenderSliverScrollingPersistentHeader");
}

#[test]
fn pinned_header_updates_stretch_configuration() {
    stretch_configuration_updates(true, "RenderSliverPinnedPersistentHeader");
}

// ─────────────────────────────────────────────────────────────────────────────
// :140 — floating: the header does not change the scroll extents
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: two 1000-high slivers around a floating 200-header in a 600
/// viewport ⇒ max scroll extent 1600, unchanged after scrolling to the end.
#[test]
fn floating_header_does_not_change_the_scroll_offset_range() {
    let controller = ScrollController::new();
    let mut laid = mount(
        &controller,
        vec![
            big_sliver(1000.0),
            SliverPersistentHeader::new(fixed_delegate())
                .floating(true)
                .into_view()
                .boxed(),
            big_sliver(1000.0),
        ],
    );

    let position = controller.position();
    assert_eq!(controller.pixels(), 0.0);
    assert_eq!(position.min_scroll_extent(), 0.0);
    assert_eq!(position.max_scroll_extent(), 1600.0);

    controller.jump_to(10_000.0);
    laid.pump();
    assert_eq!(controller.pixels(), 1600.0, "clamped to the max extent");
    assert_eq!(position.min_scroll_extent(), 0.0);
    assert_eq!(position.max_scroll_extent(), 1600.0);
}

// ─────────────────────────────────────────────────────────────────────────────
// :173 — floating: the reveal state machine, offset by offset
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: a floating min-100/max-200 header after a 1000-high sliver in a
/// 600 viewport, stepped through six settled offsets. The paint offsets and
/// visibility below are the oracle's own literals.
#[test]
fn floating_header_normal_behavior() {
    let controller = ScrollController::new();
    let mut laid = mount(
        &controller,
        vec![
            big_sliver(1000.0),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .floating(true)
                .into_view()
                .boxed(),
            big_sliver(1000.0),
        ],
    );
    let (key1, key2, key3) = (
        viewport_child(&laid, 0),
        viewport_child(&laid, 1),
        viewport_child(&laid, 2),
    );

    verify_paint_position(&laid, key1, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key2, (0.0, 1000.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 1200.0), Some(false));

    // 1000 − 600 + 200: the header's bottom reaches the viewport bottom.
    controller.jump_to(600.0);
    laid.pump();
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key2, (0.0, 400.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 600.0), Some(false));

    // 1000 − 600 + 400.
    controller.jump_to(800.0);
    laid.pump();
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key2, (0.0, 200.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 400.0), Some(true));

    // Scrolled exactly to the header's start.
    controller.jump_to(1000.0);
    laid.pump();
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 200.0), Some(true));

    // 10% into the header: it shrinks from 200 toward its min.
    controller.jump_to(1020.0);
    laid.pump();
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 180.0), Some(true));

    // Fully past: the floating header scrolls away entirely.
    controller.jump_to(1400.0);
    laid.pump();
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 0.0), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :301 — floating: no floating behavior when animating
// ─────────────────────────────────────────────────────────────────────────────

/// [`mount`] plus a [`VsyncScope`], so `ScrollController::animate_to`'s
/// curve run (driven by the scrollable's fling controller) ticks
/// deterministically under `pump_for`.
fn mount_animated(controller: &ScrollController, slivers: Vec<BoxedView>) -> LaidOut {
    let position = controller.position();
    let vsync = Vsync::new();
    let mut laid = lay_out(
        VsyncScope::new(
            vsync.clone(),
            Scrollable::new()
                .controller(controller.clone())
                .viewport_builder(Rc::new(move |_| {
                    Viewport::new(slivers.clone())
                        .position(position.clone())
                        .boxed()
                })),
        ),
        tight(800.0, 600.0),
    );
    laid.adopt_vsync(vsync);
    laid
}

/// Drive frames until the position's scroll activity ends — the oracle's
/// `pumpAndSettle(Duration(milliseconds: 1000))`: second-long virtual
/// steps, bounded so a never-settling run fails loudly instead of hanging.
fn settle_animation(laid: &mut LaidOut, controller: &ScrollController) {
    // Always pump at least once (the oracle's `pumpAndSettle` does too):
    // the queued command is serviced by the FIRST rebuild after
    // `animate_to`'s notify, so the activity signal only rises after it.
    let mut pumps = 0;
    loop {
        laid.pump_for(Duration::from_secs(1));
        pumps += 1;
        if !controller.position().is_scrolling() || pumps >= 120 {
            break;
        }
    }
    assert!(
        !controller.position().is_scrolling(),
        "the driven run must settle (still animating after {pumps} 1s pumps)"
    );
    // One trailing frame: the tick that completes the run writes the final
    // pixel value and ends the activity in the same frame, so the layout
    // that consumes that value is still pending — the oracle's
    // `pumpAndSettle` keeps pumping until NO frames are scheduled, which
    // includes it. `tick` (a frame with no root dirtying), not `pump` (a
    // `setState`-style rebuild), so the settle adds no extra build pass
    // the oracle would not have run.
    laid.tick();
}

/// Oracle: a floating header must NOT float back into view when the
/// position is moved START-WARD by `animateTo` — a driven run is not a
/// user scroll, and floating keys on the USER direction only. Both
/// animations use the oracle's own 1-minute linear runs, settled to
/// completion before asserting.
#[test]
fn floating_header_does_not_float_during_a_position_animation() {
    let controller = ScrollController::new();
    let mut laid = mount_animated(
        &controller,
        vec![
            big_sliver(1000.0),
            SliverPersistentHeader::new(fixed_delegate())
                .floating(true)
                .into_view()
                .boxed(),
            big_sliver(1000.0),
        ],
    );
    let (key1, key2, key3) = (
        viewport_child(&laid, 0),
        viewport_child(&laid, 1),
        viewport_child(&laid, 2),
    );

    verify_paint_position(&laid, key1, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key2, (0.0, 1000.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 1200.0), Some(false));

    // bigHeight + maxExtent * 2 = 1400: fully past the header.
    controller.animate_to(1400.0, Duration::from_mins(1), Arc::new(Curves::Linear));
    settle_animation(&mut laid, &controller);
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 0.0), Some(true));

    // bigHeight + maxExtent * 1.9 = 1380: 20px START-WARD — a drag in this
    // direction would float the header; a driven run must not.
    controller.animate_to(1380.0, Duration::from_mins(1), Arc::new(Curves::Linear));
    settle_animation(&mut laid, &controller);
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 0.0), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :346 — floating: floating behavior when dragging down
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: same scene as the no-float case, but with the USER direction
/// forced start-ward before the second (backward) run settles — the
/// oracle's `position.updateUserScrollDirection(ScrollDirection.forward)`,
/// FLUI's `set_user_scroll_direction`. Now the header MUST float back into
/// view part-way: its child box commits at the oracle's literal rect
/// `(0, −maxExtent·0.4, 800, maxExtent·0.5)` = `(0, −80, 800, 100)`.
#[test]
fn floating_header_floats_when_the_user_direction_is_startward() {
    let controller = ScrollController::new();
    let mut laid = mount_animated(
        &controller,
        vec![
            big_sliver(1000.0),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .floating(true)
                .into_view()
                .boxed(),
            big_sliver(1000.0),
        ],
    );
    let (key1, key2, key3) = (
        viewport_child(&laid, 0),
        viewport_child(&laid, 1),
        viewport_child(&laid, 2),
    );

    verify_paint_position(&laid, key1, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key2, (0.0, 1000.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 1200.0), Some(false));

    controller.animate_to(1400.0, Duration::from_mins(1), Arc::new(Curves::Linear));
    settle_animation(&mut laid, &controller);
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key3, (0.0, 0.0), Some(true));

    controller.animate_to(1380.0, Duration::from_mins(1), Arc::new(Curves::Linear));
    // One pump first: FLUI services the queued command (and so begins the
    // scroll activity) on the first rebuild, and `set_user_scroll_direction`
    // deliberately ignores writes outside an active scroll — where the
    // oracle's `animateTo` begins its activity synchronously, so its
    // `updateUserScrollDirection` call lands mid-activity already.
    laid.pump_for(Duration::from_secs(1));
    controller
        .position()
        .set_user_scroll_direction(ScrollDirection::Forward);
    settle_animation(&mut laid, &controller);
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_actual_box_position(&laid, laid.only_child(key2), (0.0, -80.0, 800.0, 100.0));
    verify_paint_position(&laid, key3, (0.0, 0.0), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :552 — pinned with slow scroll
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: two pinned min-100/max-200 headers stacked between 550-high
/// slivers, stepped through nine settled `animateTo` offsets; every paint
/// offset, visibility, and the second header's child-box rects are the
/// oracle's own literals. (The oracle's per-step `pumpAndSettle` always
/// runs each 1-minute animation to completion — the "slow scroll" is the
/// 50px step size, not a mid-animation state.)
#[test]
fn pinned_headers_through_a_slow_stepped_scroll() {
    let controller = ScrollController::new();
    let mut laid = mount_animated(
        &controller,
        vec![
            big_sliver(550.0),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .pinned(true)
                .into_view()
                .boxed(),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .pinned(true)
                .into_view()
                .boxed(),
            big_sliver(550.0),
            big_sliver(550.0),
        ],
    );
    let (key1, key2, key3, key4, key5) = (
        viewport_child(&laid, 0),
        viewport_child(&laid, 1),
        viewport_child(&laid, 2),
        viewport_child(&laid, 3),
        viewport_child(&laid, 4),
    );

    verify_paint_position(&laid, key1, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key2, (0.0, 550.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 750.0), Some(false));
    verify_paint_position(&laid, key4, (0.0, 950.0), Some(false));
    verify_paint_position(&laid, key5, (0.0, 1500.0), Some(false));

    let step = |laid: &mut LaidOut, target: f32| {
        controller.animate_to(target, Duration::from_mins(1), Arc::new(Curves::Linear));
        settle_animation(laid, &controller);
    };

    step(&mut laid, 550.0);
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 200.0), Some(true));
    verify_paint_position(&laid, key4, (0.0, 400.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 950.0), Some(false));

    step(&mut laid, 600.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 150.0), Some(true));
    verify_paint_position(&laid, key4, (0.0, 350.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 900.0), Some(false));

    step(&mut laid, 650.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_actual_box_position(&laid, laid.only_child(key3), (0.0, 100.0, 800.0, 200.0));
    verify_paint_position(&laid, key4, (0.0, 300.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 850.0), Some(false));

    step(&mut laid, 700.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_actual_box_position(&laid, laid.only_child(key3), (0.0, 100.0, 800.0, 200.0));
    verify_paint_position(&laid, key4, (0.0, 250.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 800.0), Some(false));

    step(&mut laid, 750.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_actual_box_position(&laid, laid.only_child(key3), (0.0, 100.0, 800.0, 200.0));
    verify_paint_position(&laid, key4, (0.0, 200.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 750.0), Some(false));

    step(&mut laid, 800.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_paint_position(&laid, key4, (0.0, 150.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 700.0), Some(false));

    step(&mut laid, 850.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_paint_position(&laid, key4, (0.0, 100.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 650.0), Some(false));

    step(&mut laid, 900.0);
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_paint_position(&laid, key4, (0.0, 50.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 600.0), Some(false));

    step(&mut laid, 950.0);
    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_actual_box_position(&laid, laid.only_child(key3), (0.0, 100.0, 800.0, 100.0));
    verify_paint_position(&laid, key4, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key5, (0.0, 550.0), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :673 — pinned with less overlap
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: 650-high slivers leave the viewport LESS than fully obstructed
/// by the two pinned headers at full scroll; max extent is exactly 1750,
/// the clamped jump lands there, and the trailing sliver becomes visible
/// at the fold.
#[test]
fn pinned_headers_with_less_overlap_at_full_scroll() {
    let controller = ScrollController::new();
    let mut laid = mount_animated(
        &controller,
        vec![
            big_sliver(650.0),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .pinned(true)
                .into_view()
                .boxed(),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .pinned(true)
                .into_view()
                .boxed(),
            big_sliver(650.0),
            big_sliver(650.0),
        ],
    );
    let (key1, key2, key3, key4, key5) = (
        viewport_child(&laid, 0),
        viewport_child(&laid, 1),
        viewport_child(&laid, 2),
        viewport_child(&laid, 3),
        viewport_child(&laid, 4),
    );

    let position = controller.position();
    // 650·3 + 200·2 − 600 (the viewport height).
    assert_eq!(controller.pixels(), 0.0);
    assert_eq!(position.min_scroll_extent(), 0.0);
    assert_eq!(position.max_scroll_extent(), 1750.0);

    controller.animate_to(10_000.0, Duration::from_mins(1), Arc::new(Curves::Linear));
    settle_animation(&mut laid, &controller);
    assert_eq!(controller.pixels(), 1750.0, "clamped to the max extent");
    assert_eq!(position.min_scroll_extent(), 0.0);
    assert_eq!(position.max_scroll_extent(), 1750.0);

    verify_paint_position(&laid, key1, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key2, (0.0, 0.0), Some(true));
    verify_paint_position(&laid, key3, (0.0, 100.0), Some(true));
    verify_paint_position(&laid, key4, (0.0, 0.0), Some(false));
    verify_paint_position(&laid, key5, (0.0, 0.0), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :759 — pointer scrolled floating
// ─────────────────────────────────────────────────────────────────────────────

/// `TestDelegate3`: min = max = 56 (a toolbar-height floating header).
struct ToolbarDelegate;

impl SliverPersistentHeaderDelegate for ToolbarDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        _shrink_offset: f32,
        _overlaps_content: bool,
    ) -> BoxedView {
        SizedBox::new(800.0, 56.0).into_view().boxed()
    }

    fn min_extent(&self) -> f32 {
        56.0
    }

    fn max_extent(&self) -> f32 {
        56.0
    }
}

/// Oracle: wheel the floating header away (+300), wheel back a little
/// (−50) — the header must FLOAT back in by exactly that much even though
/// the list is nowhere near its start — then the rest of the way (−250).
/// A wheel tick is a USER scroll: its direction pulse is what
/// `allow_floating_expansion` keys on. Deltas are the normalized
/// cross-backend convention — positive = content down — so the literals
/// below are the oracle's own `scroll` offsets, sign for sign.
#[test]
fn a_pointer_scroll_floats_the_header_back_in() {
    let controller = ScrollController::new();
    let mut laid = mount_animated(
        &controller,
        vec![
            SliverPersistentHeader::new(ToolbarDelegate)
                .floating(true)
                .into_view()
                .boxed(),
            SliverFixedExtentList::new(
                50.0,
                (0..30)
                    .map(|_| SizedBox::new(800.0, 50.0).into_view().boxed())
                    .collect::<Vec<BoxedView>>(),
            )
            .into_view()
            .boxed(),
        ],
    );
    let header = viewport_child(&laid, 0);

    assert!(laid.sliver_geometry(header).visible);
    assert_eq!(laid.sliver_geometry(header).paint_extent, 56.0);

    // Wheel the header (and the list start) away.
    laid.dispatch_scroll(400.0, 300.0, 0.0, 300.0);
    laid.pump();
    assert_eq!(laid.sliver_geometry(header).paint_extent, 0.0);
    assert!(!laid.sliver_geometry(header).visible);

    // A small wheel-up: the header floats back in by the tick's extent.
    laid.dispatch_scroll(400.0, 300.0, 0.0, -50.0);
    laid.pump();
    assert_eq!(laid.sliver_geometry(header).paint_extent, 50.0);
    assert!(laid.sliver_geometry(header).visible);

    // The rest of the way in.
    laid.dispatch_scroll(400.0, 300.0, 0.0, -250.0);
    laid.pump();
    assert_eq!(laid.sliver_geometry(header).paint_extent, 56.0);
    assert!(laid.sliver_geometry(header).visible);
}

// ─────────────────────────────────────────────────────────────────────────────
// :400 / :720 / :883 — the overscroll gap opens BELOW the header
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle (all three variants share the assertion set): over-scrolled at
/// the start, the header stays put at the top and the gap opens between it
/// and the content — never above the header.
fn overscroll_gap_is_below_header(header: SliverPersistentHeader) {
    let controller = ScrollController::new();
    let mut laid = mount(
        &controller,
        vec![header.into_view().boxed(), big_sliver(300.0)],
    );
    let (key_header, key_content) = (viewport_child(&laid, 0), viewport_child(&laid, 1));

    verify_paint_position(&laid, key_header, (0.0, 0.0), None);
    verify_paint_position(&laid, key_content, (0.0, 200.0), None);

    // The oracle's `position.jumpTo(-50.0)` is unclamped; the FLUI
    // controller clamps to the extents, so force the overscroll through
    // the position's own unclamped setter.
    controller.position().set_pixels(-50.0);
    laid.pump();

    verify_paint_position(&laid, key_header, (0.0, 0.0), None);
    verify_paint_position(&laid, key_content, (0.0, 250.0), None);
}

#[test]
fn scrolling_overscroll_gap_is_below_header() {
    overscroll_gap_is_below_header(SliverPersistentHeader::new(fixed_delegate()));
}

#[test]
fn floating_overscroll_gap_is_below_header() {
    overscroll_gap_is_below_header(SliverPersistentHeader::new(fixed_delegate()).floating(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :437 — pinned: both headers hold, stacked, at full scroll
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle literals: max scroll extent 1450; at the end the first pinned
/// header holds at the top edge (visible), the second right below it at its
/// collapsed 100, and the last big sliver peeks at y = 50.
#[test]
fn pinned_headers_hold_at_full_scroll() {
    let controller = ScrollController::new();
    let mut laid = mount(
        &controller,
        vec![
            big_sliver(550.0),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .pinned(true)
                .into_view()
                .boxed(),
            SliverPersistentHeader::new(ShrinkingDelegate)
                .pinned(true)
                .into_view()
                .boxed(),
            big_sliver(550.0),
            big_sliver(550.0),
        ],
    );
    let position = controller.position();
    assert_eq!(position.max_scroll_extent(), 1450.0);

    controller.jump_to(10_000.0);
    laid.pump();
    assert_eq!(controller.pixels(), 1450.0);

    let ids: Vec<_> = (0..5).map(|i| viewport_child(&laid, i)).collect();
    verify_paint_position(&laid, ids[0], (0.0, 0.0), Some(false));
    verify_paint_position(&laid, ids[1], (0.0, 0.0), Some(true));
    verify_paint_position(&laid, ids[2], (0.0, 100.0), Some(true));
    verify_paint_position(&laid, ids[3], (0.0, 0.0), Some(true));
    verify_paint_position(&laid, ids[4], (0.0, 50.0), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// :812 — scrolling: everything scrolls away; the tail peeks
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle literals: max 1450; at full scroll every earlier sliver paints at
/// the origin (scrolled past) and the last one peeks at y = 50.
#[test]
fn scrolling_headers_scroll_away_entirely() {
    let controller = ScrollController::new();
    let mut laid = mount(
        &controller,
        vec![
            big_sliver(550.0),
            SliverPersistentHeader::new(fixed_delegate())
                .into_view()
                .boxed(),
            SliverPersistentHeader::new(fixed_delegate())
                .into_view()
                .boxed(),
            big_sliver(550.0),
            big_sliver(550.0),
        ],
    );
    let position = controller.position();
    assert_eq!(position.max_scroll_extent(), 1450.0);

    controller.jump_to(10_000.0);
    laid.pump();
    assert_eq!(controller.pixels(), 1450.0);

    let ids: Vec<_> = (0..5).map(|i| viewport_child(&laid, i)).collect();
    for id in &ids[..4] {
        verify_paint_position(&laid, *id, (0.0, 0.0), None);
    }
    verify_paint_position(&laid, ids[4], (0.0, 50.0), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// :851 — scrolling off screen: the child's box juts above the viewport
// ─────────────────────────────────────────────────────────────────────────────

/// Oracle: scrolled to `550 + 200 − 5`, the header's CHILD box sits at
/// y = −195 with its full 200 height — the header scrolls off without
/// collapsing (min == max), so 5px of it still shows.
#[test]
fn scrolling_header_scrolls_off_screen_uncollapsed() {
    let controller = ScrollController::new();
    let delegate = fixed_delegate();
    let mut laid = mount(
        &controller,
        vec![
            big_sliver(550.0),
            SliverPersistentHeader::new(delegate).into_view().boxed(),
            big_sliver(550.0),
            big_sliver(550.0),
        ],
    );

    controller.jump_to(550.0 + 200.0 - 5.0);
    laid.pump();

    let header = viewport_child(&laid, 1);
    let child = laid.only_child(header);
    assert_eq!(
        laid.absolute_offset(child).dy.get(),
        -195.0,
        "the child box juts 195px above the viewport"
    );
    assert_eq!(
        laid.size(child).height.get(),
        200.0,
        "an uncollapsible header keeps its full extent while scrolling off"
    );
}
