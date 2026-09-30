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
//! - `should_rebuild` gates delegate swaps;
//! - all four pinned × floating variants mount and build through the seam.
//!
//! Stretch/snap configurations and `SliverAppBar` scaffolding are deferred by
//! this widget and not covered here.

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

/// A delegate swap that SHRINKS `max_extent` while scrolled beyond it. The
/// retained published pair (120) exceeds the new delegate's maximum (60),
/// so a naive swap-time rebuild would hand the new delegate an out-of-range
/// value — but an extent-changing swap routes through the layout seam: the
/// extent setters mark the child update, layout republishes the freshly
/// clamped pair, and the ONE rebuild the delegate sees carries it. Probed,
/// not assumed: exactly one post-swap build, value 60, never 120.
pub(crate) fn a_swap_that_shrinks_max_extent_never_hands_the_delegate_an_out_of_range_pair() {
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
pub(crate) fn a_floating_snap_header_snaps_fully_open_when_a_startward_scroll_ends() {
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
