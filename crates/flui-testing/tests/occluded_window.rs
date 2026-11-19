//! A window the window system keeps off the screen draws no frame while its
//! content changes. Where the occlusion may be a covering and the window
//! system keeps the covered window's surface (AppKit), the window draws one
//! owed frame once it has been quiet a while, so the surface shown on
//! uncovering holds the content of then, not of before the covering; elsewhere
//! (no covering told, a lock, a background app) it draws nothing until it is
//! seen. An owed frame stays due until it runs, however late its wake; the
//! surface refusing it ends the owing until the window is seen (ADR-0357).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_platform::{SystemNotice, WindowCommand, WindowMode};
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintContext, RenderBox};
use flui_testing::widgets::SharedSignal;
use flui_testing::{HeadlessHost, HeadlessWindow, UnshownFrame};
use flui_view::prelude::*;
use flui_view::{RenderView, StatefulView, View};

const FRAME: Duration = Duration::from_millis(16);

/// A 40 × 40 leaf that records the number it carries each time it paints.
#[derive(Debug)]
struct NumberBox {
    number: u32,
    painted: Arc<AtomicU32>,
}

impl flui_foundation::inspect::Inspect for NumberBox {
    fn kind(&self) -> &'static str {
        "NumberBox"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for NumberBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(40.0, 40.0))
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        self.painted.store(self.number, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct PaintedNumber {
    number: u32,
    painted: Arc<AtomicU32>,
}

impl RenderView for PaintedNumber {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = NumberBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> NumberBox {
        NumberBox {
            number: self.number,
            painted: Arc::clone(&self.painted),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut NumberBox,
    ) -> flui_rendering::RenderUpdateImpact {
        if render_object.number == self.number {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        render_object.number = self.number;
        flui_rendering::RenderUpdateImpact::PAINT
    }
}

impl View for PaintedNumber {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// The content: the number a test sets, painted by a [`NumberBox`].
#[derive(Clone, StatefulView)]
struct Content {
    number: SharedSignal<u32>,
    painted: Arc<AtomicU32>,
}

struct ContentState {
    number: SharedSignal<u32>,
}

impl StatefulView for Content {
    type State = ContentState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ContentState {
            number: self.number.clone(),
        }
    }
}

impl ViewState<Content> for ContentState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.number.bind(ctx);
    }

    fn build(&self, view: &Content, ctx: &dyn BuildContext) -> impl IntoView {
        PaintedNumber {
            number: self.number.get(ctx),
            painted: Arc::clone(&view.painted),
        }
    }
}

/// How the window comes back into sight.
#[derive(Clone, Copy, Debug)]
enum Uncovering {
    /// After the runtime's next wake: the quiet period, where there is one.
    AfterTheNextWake,
    /// One frame after the change, before any quiet period ends.
    AtOnce,
}

struct Case {
    name: &'static str,
    /// Whether the window system tells a covering, so the occlusion may be one.
    reports_covering: bool,
    uncovering: Uncovering,
    /// Whether the content changes while the window is occluded.
    changes: bool,
    /// Frames drawn while still occluded.
    frames_while_occluded: u64,
    /// Frames drawn once the window is seen again.
    frames_once_seen: u64,
}

/// The cases differ in whether the occlusion may be a covering, whether the
/// content changes while the window is occluded and when the window is
/// uncovered. Once seen, the window draws exactly one frame, with the content
/// of then: the only window coming back into sight turns frames back on, and
/// that redraws it in full.
///
/// The first row fails without the owed frame: the covered window draws
/// nothing until it is uncovered, so its uncovering shows the frame of before
/// the covering until the frame drawn then reaches the screen; and the loop is
/// asked for a frame at once and on every wake while covered, each drawing
/// nothing.
#[test]
fn an_occluded_window_draws_its_change_in_exactly_one_frame() {
    let cases = [
        Case {
            name: "a covering draws one owed frame once quiet",
            reports_covering: true,
            uncovering: Uncovering::AfterTheNextWake,
            changes: true,
            frames_while_occluded: 1,
            frames_once_seen: 1,
        },
        Case {
            name: "a covering lifted before the quiet period ends draws one frame then",
            reports_covering: true,
            uncovering: Uncovering::AtOnce,
            changes: true,
            frames_while_occluded: 0,
            frames_once_seen: 1,
        },
        Case {
            name: "an occlusion that is no covering draws nothing until seen",
            reports_covering: false,
            uncovering: Uncovering::AfterTheNextWake,
            changes: true,
            frames_while_occluded: 0,
            frames_once_seen: 1,
        },
        Case {
            name: "a covering with nothing changed draws nothing while covered",
            reports_covering: true,
            uncovering: Uncovering::AfterTheNextWake,
            changes: false,
            frames_while_occluded: 0,
            frames_once_seen: 1,
        },
    ];
    let failed: Vec<&str> = cases
        .iter()
        .filter(|case| std::panic::catch_unwind(|| run(case)).is_err())
        .map(|case| case.name)
        .collect();
    assert!(failed.is_empty(), "failing cases: {failed:?}");
}

/// A host showing [`Content`] at 1 in `window`, its first frame drawn: the
/// host, the number the content shows and the number it last painted.
fn mounted(window: HeadlessWindow) -> (HeadlessHost, SharedSignal<u32>, Arc<AtomicU32>) {
    let mut host = HeadlessHost::new(window);
    let number = SharedSignal::new(1);
    let painted = Arc::new(AtomicU32::new(0));
    host.attach(&Content {
        number: number.clone(),
        painted: Arc::clone(&painted),
    })
    .expect("a fresh host has no root");
    let _ = host.pump(FRAME);
    assert_eq!(painted.load(Ordering::SeqCst), 1, "the first frame");
    (host, number, painted)
}

/// A host whose window tells a covering and keeps a covered window's surface,
/// its window covered and its content changed to 2 after a frame: the owed
/// frame's quiet period runs.
fn covered_with_a_change() -> (HeadlessHost, Arc<AtomicU32>) {
    let (mut host, number, painted) = mounted(HeadlessWindow::new(40, 40).with_occlusion_reports());
    host.set_window_occluded(host.primary_window(), true);
    let _ = host.pump(FRAME);
    number.set(2);
    let _ = host.pump(FRAME);
    (host, painted)
}

fn set_mode(host: &HeadlessHost, mode: WindowMode) {
    WindowCommand::SetMode(mode)
        .admit_and_apply(host.window())
        .expect("the headless window takes a mode");
}

fn run(case: &Case) {
    let name = case.name;
    let window = HeadlessWindow::new(40, 40);
    let window = if case.reports_covering {
        window.with_occlusion_reports()
    } else {
        window
    };
    let (mut host, number, painted) = mounted(window);
    let primary = host.primary_window();

    host.set_window_occluded(primary, true);
    let _ = host.pump(FRAME);
    let before = host.sink().submits();
    let shown = if case.changes {
        number.set(2);
        2
    } else {
        1
    };
    let _ = host.pump(FRAME);
    assert_eq!(
        host.sink().submits(),
        before,
        "{name}: the change draws no frame of its own while occluded"
    );

    if matches!(case.uncovering, Uncovering::AfterTheNextWake) {
        match host.time_to_next_frame() {
            Some(wait) => {
                assert!(
                    wait > FRAME,
                    "{name}: an occluded window waits out a quiet period, not a frame: {wait:?}"
                );
                let _ = host.pump(wait);
            }
            None => assert_eq!(
                case.frames_while_occluded, 0,
                "{name}: an owed frame needs a wake"
            ),
        }
        let _ = host.pump(FRAME);
    }
    let drawn_occluded = host.sink().submits() - before;
    assert_eq!(
        drawn_occluded, case.frames_while_occluded,
        "{name}: frames while occluded"
    );
    if drawn_occluded > 0 {
        assert_eq!(
            painted.load(Ordering::SeqCst),
            shown,
            "{name}: the owed frame paints the new content"
        );
    }

    host.set_window_occluded(primary, false);
    let seen = host.sink().submits();
    let _ = host.pump(FRAME);
    let _ = host.pump(FRAME);
    assert_eq!(
        host.sink().submits() - seen,
        case.frames_once_seen,
        "{name}: frames once seen"
    );
    assert_eq!(
        painted.load(Ordering::SeqCst),
        shown,
        "{name}: the window shows its content once seen"
    );
}

/// A covered window beside a shown one that draws a frame every 16 ms: the
/// covered window is asked for its owed frame once, at the end of its quiet
/// period, however many frames the shown window draws before and after; the
/// frame served then draws the covered window's change; the request is
/// withdrawn once that frame ran with nothing left, asked again for the next
/// change at the next quiet period's end, and withdrawn when the window is
/// seen again, which draws the change then. Fails when the owed frame rides
/// the application's wake deadline: the covered window is asked for nothing.
#[test]
fn a_covered_window_beside_a_shown_one_is_asked_for_its_owed_frame_alone() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(40, 40));
    let shown = SharedSignal::new(0_u32);
    host.attach(&Content {
        number: shown.clone(),
        painted: Arc::new(AtomicU32::new(0)),
    })
    .expect("a fresh host has no root");
    let covered = host.open_window(HeadlessWindow::new(40, 40).with_occlusion_reports());
    let number = SharedSignal::new(1);
    let painted = Arc::new(AtomicU32::new(0));
    host.attach_to(
        covered,
        &Content {
            number: number.clone(),
            painted: Arc::clone(&painted),
        },
    )
    .expect("the opened window has no root");
    let _ = host.pump(FRAME);
    assert_eq!(painted.load(Ordering::SeqCst), 1, "the first frame");

    // The shown window changes before each of its frames.
    let mut shown_frame = 0;
    let mut draw_shown = |host: &mut HeadlessHost, frames: u32| {
        for _ in 0..frames {
            shown_frame += 1;
            shown.set(shown_frame);
            let _ = host.pump(FRAME);
        }
    };
    let quiet_ends = host.clock().elapsed() + Duration::from_secs(1);
    let owed = flui_platform::EventTime::from_nanos(
        u64::try_from(quiet_ends.as_nanos()).expect("BUG: a test's time fits the event timeline"),
    );
    host.set_window_occluded(covered, true);
    number.set(2);

    draw_shown(&mut host, 50);
    assert_eq!(
        host_requests(&host, covered),
        [Some(owed)],
        "asked once, at the quiet period's end, through 50 frames of the shown window"
    );
    assert_eq!(
        painted.load(Ordering::SeqCst),
        1,
        "nothing drawn before then"
    );

    draw_shown(&mut host, 20);
    assert_eq!(
        painted.load(Ordering::SeqCst),
        2,
        "the owed frame draws the change"
    );
    assert_eq!(
        host_requests(&host, covered),
        [Some(owed), None],
        "drawn with nothing left, the request is withdrawn, and the shown window's \
         frames ask nothing again"
    );

    number.set(3);
    draw_shown(&mut host, 30);
    let next = match host_requests(&host, covered)[..] {
        [Some(first), None, Some(next)] if first == owed && next > owed => next,
        ref other => panic!("the next change is asked for once, later: {other:?}"),
    };
    assert_eq!(
        painted.load(Ordering::SeqCst),
        2,
        "the next change waits for its quiet period"
    );
    host.set_window_occluded(covered, false);
    assert_eq!(
        host_requests(&host, covered),
        [Some(owed), None, Some(next), None],
        "the window seen again: its request is withdrawn"
    );
    draw_shown(&mut host, 1);
    assert_eq!(painted.load(Ordering::SeqCst), 3, "and it draws its change");
}

/// A covered window whose content changes between two pumps, with no pump,
/// observation or notice after the change: the host's loop still waits for
/// the owed frame and draws it, as a runner's owner delivery asks at the end
/// of the work item that made the change. Fails when the host asks for owed
/// frames only after its own turns: the loop finds nothing to wait for,
/// `pump_until_settled` settles at once, and the change is never drawn.
#[test]
fn a_change_between_pumps_is_still_owed_its_frame() {
    let (mut host, number, painted) = mounted(HeadlessWindow::new(40, 40).with_occlusion_reports());
    host.set_window_occluded(host.primary_window(), true);
    let _ = host.pump(FRAME);
    number.set(2);
    let wait = host
        .time_to_next_frame()
        .expect("the change leaves the covered window owing its frame");
    assert!(wait > FRAME, "after its quiet period: {wait:?}");
    let frames = host
        .pump_until_settled(FRAME, 200)
        .expect("the owed frame settles the host");
    assert!(frames > 0, "the host pumped for the owed frame");
    assert_eq!(
        painted.load(Ordering::SeqCst),
        2,
        "the owed frame draws the change"
    );
}

/// Every timed frame request `window` took, oldest first.
fn host_requests(
    host: &HeadlessHost,
    window: flui_testing::HeadlessWindowId,
) -> Vec<Option<flui_platform::EventTime>> {
    host.window_of(window).timed_frame_requests()
}

/// A covered window minimized owes nothing, and restored while still covered
/// owes its frame again, though it was never seen in between: a change of
/// sight that leaves the window unseen changes no visibility, and the owing
/// follows every snapshot. Fails when owing is decided only as the window
/// stops being seen: the minimized window keeps the quiet period it was
/// covered with, and the one restored never owes again.
#[test]
fn a_covered_window_owes_by_its_sight_on_every_snapshot() {
    let (mut host, painted) = covered_with_a_change();
    set_mode(&host, WindowMode::Minimized);
    let _ = host.pump(FRAME);
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "a minimized window owes its surface nothing"
    );
    let before = host.sink().submits();
    set_mode(&host, WindowMode::Normal);
    let _ = host.pump(FRAME);
    let wait = host
        .time_to_next_frame()
        .expect("restored while covered, the window owes its frame again");
    assert!(wait > FRAME, "after a quiet period: {wait:?}");
    let _ = host.pump(wait);
    assert_eq!(host.sink().submits() - before, 1, "one owed frame");
    assert_eq!(painted.load(Ordering::SeqCst), 2, "with the new content");
}

/// An owed frame stays due until it runs, however late the host serves its
/// wake (a timer fired late, an application the system throttles while it is
/// covered): the late wake draws it with the new content. Fails when an owed
/// frame lapses after a wall-time grace: the late wake takes a background turn
/// and the uncovering shows the content of before the covering.
#[test]
fn a_late_wake_still_draws_the_owed_frame() {
    let (mut host, painted) = covered_with_a_change();
    let wait = host
        .time_to_next_frame()
        .expect("the covered window owes its frame");
    let before = host.sink().submits();
    let _ = host.pump(wait + Duration::from_millis(150));
    assert_eq!(
        host.sink().submits() - before,
        1,
        "the owed frame runs late"
    );
    assert_eq!(painted.load(Ordering::SeqCst), 2, "with the new content");
}

/// An owed frame the surface refuses (it withdraws the window's surface) ends
/// the obligation until the window is seen again: no frame is drawn for later
/// changes, and no deadline asks for one. Fails when a refused owed frame is
/// owed again after the next quiet period.
#[test]
fn an_owed_frame_the_surface_refuses_is_not_drawn_again() {
    let (mut host, _painted) = covered_with_a_change();
    host.leave_unshown(u32::MAX, UnshownFrame::Withdrawn);
    let wait = host
        .time_to_next_frame()
        .expect("the covered window owes its frame");
    let _ = host.pump(wait);
    let refused = host.sink().submits();
    for _ in 0..5 {
        match host.time_to_next_frame() {
            Some(wait) => {
                let _ = host.pump(wait.max(FRAME));
            }
            None => break,
        }
    }
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "a refused owed frame asks for no wake"
    );
    let _ = host.pump(Duration::from_secs(5));
    assert_eq!(host.sink().submits(), refused, "and draws nothing more");
}

/// A covered window in a locked or sleeping session owes nothing: a lock
/// covers every window, and a changing window would otherwise draw once a
/// period until the session unlocks. The lock and the sleep are apart: a lock
/// still up when the machine wakes keeps the window owing nothing. The rows
/// differ in the notices the host delivers; once the session shows windows
/// again, the window owes its frame. Fails when the session's notices do not
/// reach the owing (every row but the last two), or when one flag holds both,
/// so a wake reads as an unlock and an unlock as a wake (the third and the
/// fourth).
#[test]
fn a_covered_window_owes_nothing_while_its_session_is_locked_or_asleep() {
    use SystemNotice::{SessionLocked, SessionUnlocked, Sleep, Wake};
    // (case, the notices, whether the window then owes its frame)
    let cases: [(&str, &[SystemNotice], bool); 6] = [
        ("locked", &[SessionLocked], false),
        ("asleep", &[Sleep], false),
        (
            "locked, asleep, awake",
            &[SessionLocked, Sleep, Wake],
            false,
        ),
        (
            "asleep, locked, unlocked",
            &[Sleep, SessionLocked, SessionUnlocked],
            false,
        ),
        ("locked, unlocked", &[SessionLocked, SessionUnlocked], true),
        (
            "locked, asleep, awake, unlocked",
            &[SessionLocked, Sleep, Wake, SessionUnlocked],
            true,
        ),
    ];
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(name, notices, owes)| {
            std::panic::catch_unwind(|| session_case(name, notices, *owes)).is_err()
        })
        .map(|(name, ..)| *name)
        .collect();
    assert!(failed.is_empty(), "failing cases: {failed:?}");
}

fn session_case(name: &str, notices: &[SystemNotice], owes: bool) {
    let (mut host, painted) = covered_with_a_change();
    for notice in notices {
        host.notify(*notice);
    }
    let before = host.sink().submits();
    match host.time_to_next_frame() {
        Some(wait) if owes => {
            let _ = host.pump(wait);
            assert_eq!(host.sink().submits() - before, 1, "{name}: one owed frame");
            assert_eq!(painted.load(Ordering::SeqCst), 2, "{name}: the new content");
        }
        None if !owes => {
            let _ = host.pump(Duration::from_secs(5));
            assert_eq!(host.sink().submits(), before, "{name}: nothing is drawn");
        }
        other => panic!("{name}: owes {owes}, the next frame is {other:?}"),
    }
}
