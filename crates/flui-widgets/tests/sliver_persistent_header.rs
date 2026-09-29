//! Functional tests for [`SliverPersistentHeader`] — the widget half of the
//! persistent-header build-during-layout seam.
//!
//! What these pin, deliberately at the widget level (the four render objects
//! already carry harness tests in `flui-objects`):
//!
//! - the delegate's `build` receives the header's REAL published collapse
//!   state, in the same frame layout produced it;
//! - the child genuinely REBUILDS when the shrink state changes, and does
//!   NOT rebuild when it hasn't — the edge-trigger the whole seam exists
//!   for, and the test the issue's acceptance names ("fails if the hook is
//!   re-stubbed to a no-op");
//! - `should_rebuild` gates delegate swaps the way Flutter's contract says;
//! - all four pinned × floating variants mount and build through the seam.
//!
//! The 22-case Flutter oracle
//! (`test/widgets/sliver_persistent_header_test.dart`) is a follow-up parity
//! unit — those cases lean on stretch/snap configurations and `SliverAppBar`
//! scaffolding this widget defers; porting them rides the parity-file
//! conventions (content sweep, per-case ledger) that deserve their own
//! change rather than an appendix to this one.

use std::cell::RefCell;
use std::rc::Rc;

use crate::common::{lay_out, tight};
use flui_view::BoxedView;
use flui_view::IntoView;
use flui_view::view::ViewExt;
use flui_widgets::{
    ColoredBox, CustomScrollView, SizedBox, SliverPersistentHeader, SliverPersistentHeaderDelegate,
    SliverToBoxAdapter,
};

use flui_painting::styling::Color;

/// A delegate that records every `(shrink_offset, overlaps_content)` pair its
/// `build` was called with.
struct RecordingDelegate {
    min_extent: f64,
    max_extent: f64,
    builds: Rc<RefCell<Vec<(f64, bool)>>>,
}

impl SliverPersistentHeaderDelegate for RecordingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        shrink_offset: f64,
        overlaps_content: bool,
    ) -> BoxedView {
        self.builds
            .borrow_mut()
            .push((shrink_offset, overlaps_content));
        ColoredBox::new(Color::rgb(51, 102, 204))
            .child(SizedBox::shrink())
            .into_view()
            .boxed()
    }

    fn min_extent(&self) -> f64 {
        self.min_extent
    }

    fn max_extent(&self) -> f64 {
        self.max_extent
    }
}

/// Some scrollable content after the header, so scrolling has somewhere to go
/// and `overlaps_content` has something to overlap.
fn trailing_content() -> BoxedView {
    SliverToBoxAdapter::new()
        .child(SizedBox::new(400.0, 400.0))
        .into_view()
        .boxed()
}

fn scroll_view_at(offset: f64, header: SliverPersistentHeader) -> CustomScrollView {
    CustomScrollView::new((header, trailing_content())).offset(offset)
}

/// Scrolling changes the shrink offset, and the delegate rebuilds with the
/// new pair; pumping again WITHOUT a change must not rebuild — the cell is
/// edge-triggered, and a level-triggered regression would rebuild the header
/// every frame forever.
#[test]
fn the_child_rebuilds_on_shrink_change_and_only_then() {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let delegate = |builds: &Rc<RefCell<Vec<(f64, bool)>>>| RecordingDelegate {
        min_extent: 40.0,
        max_extent: 120.0,
        builds: Rc::clone(builds),
    };

    let mut laid = lay_out(
        scroll_view_at(0.0, SliverPersistentHeader::new(delegate(&builds))),
        tight(300.0, 300.0),
    );
    assert_eq!(builds.borrow().len(), 1, "premise: one initial build");

    // Same widget shape, new offset: the header shrinks by 50.
    laid.pump_widget(scroll_view_at(
        50.0,
        SliverPersistentHeader::new(delegate(&builds)),
    ));
    let after_scroll = builds.borrow().clone();
    assert_eq!(
        after_scroll.last(),
        Some(&(50.0, false)),
        "the rebuild carries the new published pair IN THE SAME FRAME as the \
         scroll — the fixpoint drains channel-delivered dirty marks before \
         each pass, so the offset-triggered layout publishes before \
         servicing runs, not one frame later; saw {after_scroll:?}"
    );

    let count_after_scroll = builds.borrow().len();
    laid.pump();
    assert_eq!(
        builds.borrow().len(),
        count_after_scroll,
        "an unchanged frame must not rebuild the header — the seam is \
         edge-triggered on the published pair"
    );
}

/// A delegate swap that SHRINKS `max_extent` while scrolled beyond it. The
/// retained published pair (120) exceeds the new delegate's maximum (60),
/// so a naive swap-time rebuild would hand the new delegate an out-of-range
/// value — but an extent-changing swap routes through the layout seam: the
/// extent setters mark the child update, layout republishes the freshly
/// clamped pair, and the ONE rebuild the delegate sees carries it. Probed,
/// not assumed: exactly one post-swap build, value 60, never 120.
#[test]
fn a_swap_that_shrinks_max_extent_never_hands_the_delegate_an_out_of_range_pair() {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let header = |max_extent: f64, builds: &Rc<RefCell<Vec<(f64, bool)>>>| {
        SliverPersistentHeader::new(RecordingDelegate {
            min_extent: 30.0,
            max_extent,
            builds: Rc::clone(builds),
        })
        .pinned(true)
    };

    let mut laid = lay_out(
        scroll_view_at(250.0, header(120.0, &builds)),
        tight(300.0, 300.0),
    );
    let swap_point = builds.borrow().len();
    assert_eq!(
        builds.borrow().last().map(|(shrink, _)| *shrink),
        Some(120.0),
        "premise: scrolled far past the original maximum"
    );

    laid.pump_widget(scroll_view_at(250.0, header(60.0, &builds)));

    let seen = builds.borrow().clone();
    assert_eq!(
        seen[swap_point..].len(),
        1,
        "an extent-changing swap rebuilds exactly once — a second entry \
         means a swap-time rebuild ran with the stale retained pair; saw {seen:?}"
    );
    assert!(
        seen[swap_point..].iter().all(|(shrink, _)| *shrink <= 60.0),
        "no call after the swap may exceed the NEW delegate's max_extent —          the swap-triggered rebuild must clamp the retained pair; saw {seen:?}"
    );
    assert_eq!(
        seen.last().map(|(shrink, _)| *shrink),
        Some(60.0),
        "the pair that sticks is the freshly published one, clamped by          layout itself; saw {seen:?}"
    );
}

// ============================================================================
// The child-driven paint boundary must not freeze layout (issue #708)
// ============================================================================

/// A delegate whose child can size BELOW the header's layout extent: a
/// bare `ConstrainedBox` (min 100 / max 200) with no child. On the frames
/// where the child measures under the header's `layout_extent`, the pinned
/// header emits `layout_extent > paint_extent` — a geometry that violates
/// its CONTENT contract (Flutter's debug-only assert,
/// `sliver.dart:881-885`) while remaining perfectly consumable.
struct MinSizingDelegate;

impl SliverPersistentHeaderDelegate for MinSizingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        _shrink_offset: f64,
        _overlaps_content: bool,
    ) -> BoxedView {
        flui_widgets::ConstrainedBox::new(flui_rendering::constraints::BoxConstraints {
            min_width: 0.0,
            max_width: (f64::INFINITY),
            min_height: 100.0,
            max_height: 200.0,
        })
        .into_view()
        .boxed()
    }

    fn min_extent(&self) -> f64 {
        100.0
    }

    fn max_extent(&self) -> f64 {
        200.0
    }
}

/// Issue #708: before the fix, `validate_layout_output` REJECTED that
/// content-contract-violating geometry — the commit was skipped, the
/// parent saw a `SliverGeometry::ZERO` stand-in, and since every retry
/// re-violated, the header's committed geometry stayed frozen at its last
/// pre-boundary value for the rest of the app's life. The scene must be
/// driven by real animation frames (a root-dirtying `pump` masks the
/// freeze) and must cross the boundary where the child's measure first
/// falls below the header's layout extent.
#[test]
fn a_min_sizing_child_survives_the_child_driven_paint_boundary() {
    use flui_widgets::{ScrollController, Scrollable, Viewport};

    let controller = ScrollController::new();
    let position_for_viewport = controller.position();
    let big = |height: f64| -> BoxedView {
        SliverToBoxAdapter::new()
            .child(SizedBox::new(800.0, height))
            .into_view()
            .boxed()
    };
    let slivers: Vec<BoxedView> = vec![
        big(550.0),
        SliverPersistentHeader::new(MinSizingDelegate)
            .pinned(true)
            .into_view()
            .boxed(),
        SliverPersistentHeader::new(MinSizingDelegate)
            .pinned(true)
            .into_view()
            .boxed(),
        big(550.0),
        big(550.0),
    ];
    let scrollable = Scrollable::new()
        .controller(controller.clone())
        .viewport_builder(Rc::new(move |_| {
            Viewport::new(slivers.clone())
                .position(position_for_viewport.clone())
                .boxed()
        }));
    let vsync = flui_animation::Vsync::new();
    let mut laid = lay_out(
        flui_widgets::VsyncScope::new(vsync.clone(), scrollable),
        tight(800.0, 600.0),
    );
    laid.adopt_vsync(vsync);

    // The FIRST header in scroll order — `find_all_by_render_type` returns
    // render-tree order, not paint order, so pick by committed offset.
    let header = *laid
        .find_all_by_render_type("RenderSliverPinnedPersistentHeader")
        .iter()
        .min_by(|a, b| laid.offset(**a).dy.total_cmp(&laid.offset(**b).dy))
        .expect("two pinned headers are mounted");
    // Premise: below the boundary the header's paint is capped by the
    // remaining room (600 − 550 = 50), not the child.
    assert_eq!(laid.sliver_geometry(header).paint_extent, 50.0);

    // March across the boundary (remaining exceeds the child's 100 once
    // pixels pass 50) the way a scroll animation does: the fling
    // controller's vsync tick writes pixels DURING the frame.
    controller.animate_to(
        550.0,
        std::time::Duration::from_mins(1),
        std::sync::Arc::new(flui_animation::Curves::Linear),
    );
    for _ in 0..64 {
        laid.pump_for(std::time::Duration::from_secs(1));
        if !controller.position().is_scrolling() {
            break;
        }
    }
    laid.tick();

    // pixels = 120 ⇒ remaining = 170 ⇒ the committed paint extent is the
    // CHILD's 100 — and, critically, layout must still be tracking at all.
    assert_eq!(
        laid.sliver_geometry(header).paint_extent,
        100.0,
        "the committed geometry must track across the child-driven boundary; a stale pre-boundary value means the viewport froze"
    );
    assert!(
        laid.sliver_geometry(header).visible,
        "a 100px-painted pinned header is visible"
    );
}

// ============================================================================
// Snap: the full seam — gesture end → activity signal → epoch command →
// render snap animation
// ============================================================================

/// A snapping delegate: records builds and declares a fast snap so the test
/// pumps few frames.
struct SnappingDelegate {
    builds: Rc<RefCell<Vec<(f64, bool)>>>,
}

impl SliverPersistentHeaderDelegate for SnappingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        shrink_offset: f64,
        overlaps_content: bool,
    ) -> BoxedView {
        self.builds
            .borrow_mut()
            .push((shrink_offset, overlaps_content));
        SizedBox::new(300.0, 10.0).into_view().boxed()
    }

    fn min_extent(&self) -> f64 {
        40.0
    }

    fn max_extent(&self) -> f64 {
        120.0
    }

    fn snap_configuration(&self) -> Option<flui_widgets::FloatingHeaderSnapConfiguration> {
        Some(flui_widgets::FloatingHeaderSnapConfiguration::new(
            flui_animation::ArcCurve::new(flui_animation::Curves::Linear),
            std::time::Duration::from_millis(64),
        ))
    }
}

/// The whole snap seam, end to end: scroll the header away, then end a
/// start-ward drag — the activity signal's end transition must stamp a snap
/// command, and the floating header must animate to FULLY revealed
/// (`shrink_offset == 0`) even though the scroll offset itself stays deep.
/// Snapping is reveal animation, not scroll-to-top.
#[test]
fn a_floating_snap_header_snaps_fully_open_when_a_startward_scroll_ends() {
    use std::time::Duration;

    use flui_animation::Vsync;
    use flui_widgets::{ScrollController, Scrollable, Viewport, VsyncScope};

    let builds = Rc::new(RefCell::new(Vec::new()));
    let builds_for_delegate = Rc::clone(&builds);
    let controller = ScrollController::new();
    let vsync = Vsync::new();

    let scrollable = Scrollable::new()
        .controller(controller.clone())
        .viewport_builder(Rc::new(move |position| {
            Viewport::new((
                SliverPersistentHeader::new(SnappingDelegate {
                    builds: Rc::clone(&builds_for_delegate),
                })
                .floating(true),
                trailing_content(),
            ))
            .position(position)
            .boxed()
        }));

    let mut laid = lay_out(
        VsyncScope::new(vsync.clone(), scrollable),
        tight(300.0, 300.0),
    );
    laid.adopt_vsync(vsync);

    // Scroll deep: the floating header scrolls away entirely.
    controller.jump_to(200.0);
    laid.pump();
    assert_eq!(
        builds.borrow().last().map(|(shrink, _)| *shrink),
        Some(120.0),
        "premise: the header is fully collapsed after the deep scroll"
    );

    // A small START-WARD drag (finger moving down = revealing earlier
    // content = Forward), released without fling velocity: the release is
    // what must trigger the snap.
    laid.dispatch_pointer_down(150.0, 100.0);
    laid.dispatch_pointer_move(150.0, 170.0); // 70px down: slop + pan_start
    laid.dispatch_pointer_move(150.0, 175.0); // small further drag
    laid.dispatch_pointer_up(150.0, 175.0);

    // Drive frames: whatever the release produced (immediate end or a brief
    // ballistic run), the snap animation must then expand the header to
    // fully revealed. Bounded so a never-snapping regression fails loudly.
    let mut frames = 0;
    while builds.borrow().last().map(|(shrink, _)| *shrink) != Some(0.0) && frames < 2_000 {
        laid.pump_for(Duration::from_millis(16));
        frames += 1;
    }
    assert_eq!(
        builds.borrow().last().map(|(shrink, _)| *shrink),
        Some(0.0),
        "the snap must animate the header to fully revealed (still not after \
         {frames} frames); builds: {:?}",
        builds.borrow()
    );
    assert!(
        controller.pixels() > 100.0,
        "snap is reveal animation, not scroll-to-top — the offset must stay \
         deep; got {}",
        controller.pixels()
    );
}

/// A new drag beginning mid-snap must STOP the snap immediately — the
/// finger owns the header now. Without the stop, the settle animation
/// keeps driving the reveal underneath the user's drag and the header
/// fights the gesture all the way to fully open.
#[test]
fn a_new_drag_stops_an_in_flight_snap() {
    use std::time::Duration;

    use flui_animation::Vsync;
    use flui_widgets::{ScrollController, Scrollable, Viewport, VsyncScope};

    let builds = Rc::new(RefCell::new(Vec::new()));
    let builds_for_delegate = Rc::clone(&builds);
    let controller = ScrollController::new();
    let vsync = Vsync::new();

    let scrollable = Scrollable::new()
        .controller(controller.clone())
        .viewport_builder(Rc::new(move |position| {
            Viewport::new((
                SliverPersistentHeader::new(SnappingDelegate {
                    builds: Rc::clone(&builds_for_delegate),
                })
                .floating(true),
                trailing_content(),
            ))
            .position(position)
            .boxed()
        }));

    let mut laid = lay_out(
        VsyncScope::new(vsync.clone(), scrollable),
        tight(300.0, 300.0),
    );
    laid.adopt_vsync(vsync);

    controller.jump_to(200.0);
    laid.pump();

    // End a start-ward drag: the snap toward fully-open begins.
    laid.dispatch_pointer_down(150.0, 100.0);
    laid.dispatch_pointer_move(150.0, 170.0);
    laid.dispatch_pointer_move(150.0, 175.0);
    laid.dispatch_pointer_up(150.0, 175.0);
    // One frame: the snap is in flight but far from settled (64ms config).
    laid.pump_for(Duration::from_millis(16));
    let mid_snap = builds.borrow().last().map(|(shrink, _)| *shrink);
    assert!(
        mid_snap.is_some_and(|shrink| shrink > 0.0),
        "premise: the snap is mid-flight, not settled; saw {mid_snap:?}"
    );

    // A NEW drag begins and HOLDS — the snap must yield to the finger.
    laid.dispatch_pointer_down(150.0, 100.0);
    laid.dispatch_pointer_move(150.0, 130.0); // crosses slop: pan_start
    for _ in 0..12 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        builds
            .borrow()
            .last()
            .is_some_and(|(shrink, _)| *shrink > 0.0),
        "the stopped snap must not keep animating the header open under \
         the user's finger; builds: {:?}",
        builds.borrow()
    );
}
