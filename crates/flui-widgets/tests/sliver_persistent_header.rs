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
//! - all four pinned × floating variants mount and build through the seam;
//! - a stretching header runs its `on_stretch_trigger` once per crossing of
//!   the trigger offset while a finger over-scrolls the view.
//!
//! Floating snap tests exercise gesture release, registry migration and
//! withdrawal during an active run through the mounted widget.

use flui_widgets::ScrollView;
use std::cell::RefCell;
use std::rc::Rc;

use crate::common::{lay_out, tight};
use flui_view::BoxedView;
use flui_view::IntoView;
use flui_view::ViewExt;
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
    CustomScrollView::new((header, trailing_content())).controller(
        flui_widgets::ScrollController::with_initial_scroll_offset(offset),
    )
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
            flui_animation::SharedCurve::new(flui_animation::curves::LINEAR),
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
    exercise_floating_snap(false, false);
}

pub(crate) fn an_active_floating_snap_migrates_and_unmounts() {
    for pinned in [false, true] {
        exercise_floating_snap(pinned, true);
    }
}

fn exercise_floating_snap(pinned: bool, migrate_and_unmount: bool) {
    use std::time::Duration;

    use flui_animation::Vsync;
    use flui_view::environment::VsyncScope;
    use flui_widgets::{ScrollController, ScrollView, Scrollable, Viewport};

    let builds = Rc::new(RefCell::new(Vec::new()));
    let builds_for_delegate = Rc::clone(&builds);
    let controller = ScrollController::new();
    let vsync = Vsync::new();

    let scrollable = Scrollable::new(move |position| {
        Viewport::new(
            position,
            (
                SliverPersistentHeader::new(SnappingDelegate {
                    builds: Rc::clone(&builds_for_delegate),
                })
                .pinned(pinned)
                .floating(true),
                trailing_content(),
            ),
        )
        .boxed()
    })
    .controller(controller.clone());

    let mut laid = lay_out(
        VsyncScope::new(vsync.clone(), scrollable.clone()),
        tight(300.0, 300.0),
    );
    laid.adopt_vsync(vsync.clone()).expect("clock admission");

    // Scroll deep: the floating header scrolls away entirely.
    controller.jump_to(200.0).expect("no build is running");
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
    // The sole pan member wins the closed arena at Down. Both moves are
    // accepted updates, so their full displacement reaches the scroll offset.
    laid.dispatch_pointer_move(150.0, 170.0);
    laid.tick();
    assert_eq!(
        controller.pixels(),
        130.0,
        "premise: the sole accepted pan delivers the first 70px update"
    );
    laid.dispatch_pointer_move(150.0, 175.0); // small further drag
    laid.tick();
    assert_eq!(
        controller.pixels(),
        125.0,
        "premise: the accepted drag moves toward the start"
    );
    assert!(
        controller.position().is_scrolling(),
        "premise: the drag is active"
    );
    assert_eq!(
        controller.position().user_scroll_direction(),
        flui_rendering::view::ScrollDirection::Forward,
        "premise: the snap listener has a startward user direction"
    );
    // Hardware timestamps follow the virtual clock. A stationary gap makes
    // this a release without a fling, rather than two rapid samples at Up.
    laid.pump_for(Duration::from_millis(100));
    laid.dispatch_pointer_up(150.0, 175.0);
    assert!(
        !controller.position().is_scrolling(),
        "premise: the stationary release ends scrolling"
    );

    if migrate_and_unmount {
        laid.pump_for(Duration::from_millis(16));
        laid.pump_for(Duration::from_millis(16));
        let before = builds.borrow().last().expect("header built").0;
        assert!(
            before > 0.0 && before < 120.0,
            "snap is in flight: {before}"
        );
        assert!(vsync.has_running(), "a real snap owns frame delivery");

        let next = Vsync::new();
        laid.pump_widget(VsyncScope::new(next.clone(), scrollable));
        assert!(vsync.is_empty(), "migration withdraws the preceding seats");
        laid.adopt_vsync(next.clone())
            .expect("replacement clock admission");
        laid.pump_for(Duration::ZERO);
        let migrated = builds.borrow().last().expect("header built").0;
        assert!(
            (migrated - before).abs() < 1e-8,
            "migration preserves the displayed header: {before} -> {migrated}"
        );
        laid.pump_for(Duration::from_millis(16));
        let advanced = builds.borrow().last().expect("header built").0;
        assert!(
            advanced < migrated && advanced > 0.0,
            "new registry advances the same unfinished snap: {migrated} -> {advanced}"
        );
        assert!(next.has_running());

        laid.pump_widget(SizedBox::shrink());
        assert!(
            next.is_empty(),
            "unmount withdraws the active header and scroll owner"
        );
        let retired_builds = builds.borrow().len();
        laid.pump_for(Duration::from_secs(1));
        assert_eq!(
            builds.borrow().len(),
            retired_builds,
            "retired snap cannot rebuild its delegate"
        );
        assert!(!next.has_running());
        return;
    }

    // Drive the snap after the idle release until the header is fully
    // revealed. Bounded so a never-snapping regression fails loudly.
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

// ============================================================================
// Stretch: over-scroll past the trigger offset runs `on_stretch_trigger`
// ============================================================================

/// The over-scroll, in logical pixels, at which the stretching delegate's
/// trigger runs.
const STRETCH_TRIGGER_OFFSET: f64 = 40.0;

/// A delegate whose header stretches when the scroll view over-scrolls at its
/// start, with a trigger that counts its calls.
struct StretchingDelegate {
    calls: Rc<std::cell::Cell<u32>>,
}

impl SliverPersistentHeaderDelegate for StretchingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        _shrink_offset: f64,
        _overlaps_content: bool,
    ) -> BoxedView {
        SizedBox::shrink().into_view().boxed()
    }

    fn min_extent(&self) -> f64 {
        40.0
    }

    fn max_extent(&self) -> f64 {
        100.0
    }

    fn stretch_configuration(&self) -> Option<flui_widgets::OverScrollHeaderStretchConfiguration> {
        let calls = Rc::clone(&self.calls);
        Some(flui_widgets::OverScrollHeaderStretchConfiguration::new(
            STRETCH_TRIGGER_OFFSET,
            Some(flui_widgets::StretchTrigger::new(move || {
                calls.set(calls.get() + 1);
            })),
        ))
    }
}

/// A finger pulls the scroll view past its start, so every variant's header
/// stretches: the trigger runs once on the frame the stretch reaches the
/// offset, not again while it stays past it, and once more only after the
/// stretch fell back below the offset and crossed it again.
pub(crate) fn a_stretching_header_runs_its_trigger_once_per_crossing_of_the_offset() {
    use flui_animation::Vsync;
    use flui_view::environment::VsyncScope;
    use flui_widgets::{BouncingScrollPhysics, ScrollController};

    for (pinned, floating) in [(false, false), (true, false), (false, true), (true, true)] {
        let variant = format!("pinned {pinned}, floating {floating}");
        let calls = Rc::new(std::cell::Cell::new(0_u32));
        let controller = ScrollController::new();
        let vsync = Vsync::new();
        let view = CustomScrollView::new((
            SliverPersistentHeader::new(StretchingDelegate {
                calls: Rc::clone(&calls),
            })
            .pinned(pinned)
            .floating(floating),
            trailing_content(),
        ))
        .controller(controller.clone())
        .physics(Rc::new(BouncingScrollPhysics::new()));
        let mut laid = crate::common::lay_out_animated(
            VsyncScope::new(vsync.clone(), view),
            tight(300.0, 300.0),
            vsync,
        );

        laid.dispatch_pointer_down(150.0, 50.0);
        let mut pull_to = |y: f64| {
            laid.dispatch_pointer_move(150.0, y);
            laid.tick();
            -controller.pixels()
        };

        let stretch = pull_to(80.0);
        assert!(
            stretch > 0.0 && stretch < STRETCH_TRIGGER_OFFSET,
            "{variant}: premise: a short pull stretches below the offset; stretch {stretch}"
        );
        assert_eq!(calls.get(), 0, "{variant}: no call below the offset");

        let stretch = pull_to(200.0);
        assert!(
            stretch >= STRETCH_TRIGGER_OFFSET,
            "{variant}: premise: a long pull stretches past the offset; stretch {stretch}"
        );
        assert_eq!(calls.get(), 1, "{variant}: one call on the crossing frame");

        let stretch = pull_to(260.0);
        assert!(
            stretch >= STRETCH_TRIGGER_OFFSET,
            "{variant}: premise: still past the offset; stretch {stretch}"
        );
        assert_eq!(calls.get(), 1, "{variant}: no call while it stays past");

        let stretch = pull_to(200.0);
        assert!(
            stretch > 0.0 && stretch < STRETCH_TRIGGER_OFFSET,
            "{variant}: premise: easing back drops below the offset; stretch {stretch}"
        );
        assert_eq!(calls.get(), 1, "{variant}: dropping below runs nothing");

        let stretch = pull_to(295.0);
        assert!(
            stretch >= STRETCH_TRIGGER_OFFSET,
            "{variant}: premise: the second pull crosses again; stretch {stretch}"
        );
        assert_eq!(
            calls.get(),
            2,
            "{variant}: the second crossing calls once more"
        );
    }
}
