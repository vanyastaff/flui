//! [`HeadlessHost`]'s failure contract: a frame failure the realm contains
//! is raised after the pump, the first failure of a pump stays authoritative,
//! and the realm keeps producing frames once the cause is gone.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintCx, RenderBox};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::{RenderView, View};

/// A 40 × 24 leaf whose paint panics while `armed` is set. A paint panic is
/// what the pipeline refuses the frame for (`RenderError::Poisoned`); a
/// layout panic it contains below the root with stand-in geometry.
#[derive(Debug)]
struct TripwireBox {
    armed: Arc<AtomicBool>,
}

impl flui_foundation::Diagnosticable for TripwireBox {}

impl RenderBox for TripwireBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(40.0, 24.0)
    }

    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {
        assert!(!self.armed.load(Ordering::SeqCst), "tripwire paint fired");
    }
}

#[derive(Clone)]
struct Tripwire {
    armed: Arc<AtomicBool>,
}

impl RenderView for Tripwire {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = TripwireBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> TripwireBox {
        TripwireBox {
            armed: Arc::clone(&self.armed),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut TripwireBox,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for Tripwire {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// A realm with a tripwire root, armed as asked, before its first frame.
fn tripwire_realm(armed: bool) -> (HeadlessHost, Arc<AtomicBool>) {
    let armed = Arc::new(AtomicBool::new(armed));
    let realm = HeadlessHost::new(HeadlessWindow::new(40, 24));
    realm
        .attach(&Tripwire {
            armed: Arc::clone(&armed),
        })
        .expect("a fresh realm has no root yet");
    (realm, armed)
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

/// A paint panic the realm contains as a dropped frame is raised after the
/// pump, with the pipeline's report.
///
/// Fails against a driver that returns the pump's outcome as is: the realm
/// contains the panic, the pump returns normally and nothing is raised.
#[test]
fn a_contained_frame_failure_is_raised_after_the_pump() {
    let (mut realm, _armed) = tripwire_realm(true);

    let raised = catch_unwind(AssertUnwindSafe(|| realm.pump(Duration::ZERO)))
        .expect_err("the contained paint panic must be raised");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed") && text.contains("paint"),
        "the raised panic carries the realm's pipeline report, got {text:?}"
    );
    assert_eq!(realm.sink().submits(), 0, "a dropped frame submits nothing");
}

/// Once the cause is gone, the next pump paints: the raise left the realm
/// able to make progress, and the failure is not raised again.
///
/// Fails against a driver that keeps a raised report and raises it on every
/// later pump, or that leaves the realm unable to frame after the raise.
#[test]
fn the_realm_makes_progress_after_a_raised_failure() {
    let (mut realm, armed) = tripwire_realm(true);
    let _ = catch_unwind(AssertUnwindSafe(|| realm.pump(Duration::ZERO)))
        .expect_err("the armed tripwire fails the first frame");

    armed.store(false, Ordering::SeqCst);
    let outcome = realm.pump(Duration::ZERO);

    assert!(outcome.presented(), "the retried frame presents");
    assert_eq!(realm.sink().submits(), 1);
    assert!(
        realm.sink().layer_tree().is_some(),
        "the sink keeps the scene the retried frame composited"
    );
}

/// Schedule a post-frame callback that panics, through the realm's own
/// owner-local lane.
fn schedule_post_frame_panic(realm: &HeadlessHost) {
    realm
        .local_post_frame_handle()
        .schedule_local(|_timing| panic!("post-frame callback panicked"))
        .expect("the realm's post-frame lane is alive");
}

/// After a pump that unwound, the next pump runs: it returns, and the
/// post-frame lane the unwind went through still runs a callback. A frame is
/// requested first, so the frame latch the unwind left behind must let it
/// through.
fn assert_progress_after_unwind(realm: &mut HeadlessHost) -> flui_runtime::pump::FrameOutcome {
    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_callback = Arc::clone(&ran);
    realm
        .local_post_frame_handle()
        .schedule_local(move |_timing| ran_in_callback.store(true, Ordering::SeqCst))
        .expect("the post-frame lane survives the unwind");
    realm.request_frame();

    let outcome = realm.pump(Duration::ZERO);

    assert!(
        ran.load(Ordering::SeqCst),
        "the frame after the unwind runs its post-frame callbacks"
    );
    outcome
}

/// A panic that unwinds out of the pump after the realm contained a failure
/// in the same pump does not replace it: the contained failure is raised.
/// Once the cause is gone, the realm frames again.
///
/// Fails against a driver that resumes the later unwind (the post-frame
/// callback's text would be raised) or that loses the report when the pump
/// unwinds.
#[test]
fn a_contained_failure_stays_authoritative_over_a_later_unwind() {
    let (mut realm, armed) = tripwire_realm(true);
    schedule_post_frame_panic(&realm);

    let raised = catch_unwind(AssertUnwindSafe(|| realm.pump(Duration::ZERO)))
        .expect_err("the pump fails twice and raises once");

    let text = panic_text(&*raised);
    assert!(
        text.contains("frame pipeline failed"),
        "the first failure of the pump is raised, got {text:?}"
    );
    assert!(
        !text.contains("post-frame callback panicked"),
        "the later unwind must not replace the first failure, got {text:?}"
    );

    armed.store(false, Ordering::SeqCst);
    let outcome = assert_progress_after_unwind(&mut realm);
    assert!(
        outcome.presented(),
        "the tripwire, still waiting for paint, presents once disarmed"
    );
    assert_eq!(realm.sink().submits(), 1);
}

/// With nothing contained, a panic that unwinds out of the pump is raised as
/// itself, and the realm frames again afterwards: the unwind left its frame
/// latch and post-frame lane usable.
///
/// Fails against a driver that swallows an unwind it has no report for.
#[test]
fn an_uncontained_unwind_is_raised_as_itself() {
    let (mut realm, _armed) = tripwire_realm(false);
    let _ = realm.pump(Duration::ZERO);
    schedule_post_frame_panic(&realm);
    realm.request_frame();

    let raised = catch_unwind(AssertUnwindSafe(|| realm.pump(Duration::ZERO)))
        .expect_err("the post-frame panic unwinds out of the pump");

    assert_eq!(panic_text(&*raised), "post-frame callback panicked");
    let _outcome = assert_progress_after_unwind(&mut realm);
}
