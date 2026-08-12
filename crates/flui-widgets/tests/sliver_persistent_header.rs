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

use flui_types::Color;

/// A delegate that records every `(shrink_offset, overlaps_content)` pair its
/// `build` was called with.
struct RecordingDelegate {
    min_extent: f32,
    max_extent: f32,
    builds: Rc<RefCell<Vec<(f32, bool)>>>,
}

impl SliverPersistentHeaderDelegate for RecordingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        shrink_offset: f32,
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

    fn min_extent(&self) -> f32 {
        self.min_extent
    }

    fn max_extent(&self) -> f32 {
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

fn scroll_view_at(offset: f32, header: SliverPersistentHeader) -> CustomScrollView {
    CustomScrollView::new((header, trailing_content())).offset(offset)
}

/// The first frame builds the child with the header fully expanded —
/// `shrink_offset == 0.0` — and with the real published pair, never a guess:
/// the build happens in the same frame's fixpoint, after layout published.
#[test]
fn first_frame_builds_with_the_published_expanded_state() {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let header = SliverPersistentHeader::new(RecordingDelegate {
        min_extent: 40.0,
        max_extent: 120.0,
        builds: Rc::clone(&builds),
    });

    lay_out(scroll_view_at(0.0, header), tight(300.0, 300.0));

    assert_eq!(
        builds.borrow().as_slice(),
        &[(0.0, false)],
        "one build, with the real expanded pair — a seam that guesses or \
         never fires produces zero or different entries"
    );
}

/// Scrolling changes the shrink offset, and the delegate rebuilds with the
/// new pair; pumping again WITHOUT a change must not rebuild — the cell is
/// edge-triggered, and a level-triggered regression would rebuild the header
/// every frame forever.
#[test]
fn the_child_rebuilds_on_shrink_change_and_only_then() {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let delegate = |builds: &Rc<RefCell<Vec<(f32, bool)>>>| RecordingDelegate {
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

/// A shrink offset past `max_extent` is clamped: the delegate never sees a
/// shrink larger than the header can actually collapse.
#[test]
fn shrink_offset_is_clamped_to_max_extent() {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let header = SliverPersistentHeader::new(RecordingDelegate {
        min_extent: 40.0,
        max_extent: 120.0,
        builds: Rc::clone(&builds),
    })
    .pinned(true);

    lay_out(scroll_view_at(250.0, header), tight(300.0, 300.0));

    let seen = builds.borrow().clone();
    assert_eq!(
        seen.last().map(|(shrink, _)| *shrink),
        Some(120.0),
        "shrink_offset = min(scroll_offset, max_extent); saw {seen:?}"
    );
}

/// All four variants mount and build through the seam. A variant whose
/// element failed to register its cell would produce zero builds — silently
/// static content, exactly the gap this widget existed to close.
#[test]
fn every_variant_builds_through_the_seam() {
    for (pinned, floating) in [(false, false), (true, false), (false, true), (true, true)] {
        let builds = Rc::new(RefCell::new(Vec::new()));
        let header = SliverPersistentHeader::new(RecordingDelegate {
            min_extent: 40.0,
            max_extent: 120.0,
            builds: Rc::clone(&builds),
        })
        .pinned(pinned)
        .floating(floating);

        lay_out(scroll_view_at(0.0, header), tight(300.0, 300.0));

        assert_eq!(
            builds.borrow().len(),
            1,
            "variant (pinned={pinned}, floating={floating}) must build its \
             child through the seam exactly once on the first frame"
        );
    }
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
    let header = |max_extent: f32, builds: &Rc<RefCell<Vec<(f32, bool)>>>| {
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

/// A delegate carrying an over-scroll stretch configuration.
struct StretchingDelegate {
    signal: flui_objects::StretchTriggerSignal,
}

impl SliverPersistentHeaderDelegate for StretchingDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        _shrink_offset: f32,
        _overlaps_content: bool,
    ) -> BoxedView {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }

    fn min_extent(&self) -> f32 {
        40.0
    }

    fn max_extent(&self) -> f32 {
        120.0
    }

    fn stretch_configuration(&self) -> Option<flui_objects::OverScrollHeaderStretchConfiguration> {
        Some(flui_objects::OverScrollHeaderStretchConfiguration::new(
            20.0,
            Some(self.signal.clone()),
        ))
    }
}

/// The delegate's stretch configuration reaches the render object and fires
/// its trigger when the scroll view over-scrolls past the trigger offset —
/// the whole point of plumbing the configuration through the delegate.
#[test]
fn the_delegates_stretch_configuration_fires_its_trigger_on_over_scroll() {
    let signal = flui_objects::StretchTriggerSignal::new();
    let header = SliverPersistentHeader::new(StretchingDelegate {
        signal: signal.clone(),
    });

    let mut laid = lay_out(scroll_view_at(0.0, header), tight(300.0, 300.0));
    assert_eq!(signal.count(), 0, "premise: no over-scroll yet");

    // Over-scroll past the 20px trigger.
    laid.pump_widget(scroll_view_at(
        -40.0,
        SliverPersistentHeader::new(StretchingDelegate {
            signal: signal.clone(),
        }),
    ));

    assert!(
        signal.count() >= 1,
        "over-scrolling past the trigger offset must fire the delegate's          stretch trigger signal; count = {}",
        signal.count()
    );
}

/// A delegate that delegates `should_rebuild` to a flag, counting builds.
struct GatedDelegate {
    rebuild: bool,
    builds: Rc<RefCell<Vec<(f32, bool)>>>,
}

impl SliverPersistentHeaderDelegate for GatedDelegate {
    fn build(
        &self,
        _ctx: &dyn flui_view::BuildContext,
        shrink_offset: f32,
        overlaps_content: bool,
    ) -> BoxedView {
        self.builds
            .borrow_mut()
            .push((shrink_offset, overlaps_content));
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }

    fn min_extent(&self) -> f32 {
        40.0
    }

    fn max_extent(&self) -> f32 {
        120.0
    }

    fn should_rebuild(&self, _old: &dyn SliverPersistentHeaderDelegate) -> bool {
        self.rebuild
    }
}

/// Swapping in a distinct delegate asks the NEW delegate whether the swap is
/// observable — `false` keeps the existing child, `true` rebuilds. Flutter's
/// `SliverPersistentHeaderDelegate.shouldRebuild` contract.
#[test]
fn a_delegate_swap_rebuilds_only_when_should_rebuild_says_so() {
    let builds = Rc::new(RefCell::new(Vec::new()));
    let header = |rebuild: bool, builds: &Rc<RefCell<Vec<(f32, bool)>>>| {
        SliverPersistentHeader::new(GatedDelegate {
            rebuild,
            builds: Rc::clone(builds),
        })
    };

    let mut laid = lay_out(
        scroll_view_at(0.0, header(false, &builds)),
        tight(300.0, 300.0),
    );
    assert_eq!(builds.borrow().len(), 1, "premise: one initial build");

    // A distinct delegate that declares the swap unobservable: no rebuild.
    laid.pump_widget(scroll_view_at(0.0, header(false, &builds)));
    assert_eq!(
        builds.borrow().len(),
        1,
        "should_rebuild == false must keep the existing child"
    );

    // A distinct delegate that declares it observable: rebuild.
    laid.pump_widget(scroll_view_at(0.0, header(true, &builds)));
    assert_eq!(
        builds.borrow().len(),
        2,
        "should_rebuild == true must rebuild the child"
    );
}
