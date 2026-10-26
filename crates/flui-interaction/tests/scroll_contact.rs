//! A phased touchpad pan opens an arena contact for the pointer targets that
//! declared they feed a recognizer (ADR-0251).

use std::{cell::RefCell, rc::Rc};

use flui_foundation::geometry::{Axis, Point};
use flui_interaction::{
    DragAxis, DragGestureRecognizer, EventPropagation, GestureBinding, GestureRecognizer,
    HitTestEntry, HitTestResult, InteractionLane, PointerTarget, RenderId, ScrollTarget,
};
use flui_platform::{
    EventTime, PointerEvent,
    pointer::{
        DeviceId, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta, ScrollEvent,
        ScrollPhase, ScrollPrecision, ScrollUnit,
    },
};

/// What a recording handler saw of a contact.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Down(Point<f64>),
    Move(Point<f64>),
    Up(Point<f64>),
    Cancel,
}

fn source() -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(3_u64).expect("pointer"),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(1_u64).expect("device"))
}

fn packet(phase: ScrollPhase, precision: ScrollPrecision, dy: f64, nanos: u64) -> PointerEvent {
    PointerEvent::Scroll(
        ScrollEvent::new(
            source(),
            EventTime::from_nanos(nanos),
            PointerPosition::try_new(Point::new(50.0, 60.0)).expect("position"),
            ScrollDelta::try_new(ScrollUnit::Pixels, 0.0, dy).expect("delta"),
        )
        .with_precision(precision)
        .with_phase(phase),
    )
}

fn pan(phase: ScrollPhase, dy: f64, nanos: u64) -> PointerEvent {
    packet(phase, ScrollPrecision::Precise, dy, nanos)
}

struct Rig {
    lane: InteractionLane,
    binding: GestureBinding,
    seen: Rc<RefCell<Vec<Seen>>>,
    scrolled: Rc<RefCell<Vec<f64>>>,
    path: HitTestResult,
}

/// A path with one pointer handler (declared a feed or not) and one scroll
/// target beneath it.
fn rig(declared: bool) -> Rig {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let scrolled = Rc::new(RefCell::new(Vec::new()));
    let mut path = HitTestResult::new();
    lane.enter(|| {
        let log = Rc::clone(&seen);
        let feed: PointerTarget = handle
            .register_pointer(move |dispatch| {
                let entry = match dispatch.global {
                    PointerEvent::Down(down) => Seen::Down(down.sample.position.get()),
                    PointerEvent::Move(motion) => Seen::Move(motion.current().position.get()),
                    PointerEvent::Up(up) => Seen::Up(up.sample.position.get()),
                    PointerEvent::Cancel(_) => Seen::Cancel,
                    _ => return,
                };
                log.borrow_mut().push(entry);
            })
            .expect("pointer handler");
        if declared {
            handle
                .declare_scroll_contact_feed(feed)
                .expect("declaration");
        }
        let log = Rc::clone(&scrolled);
        let scroll: ScrollTarget = handle
            .register_scroll(move |event| {
                log.borrow_mut().push(event.delta.y());
                EventPropagation::Stop
            })
            .expect("scroll target");
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(feed));
        path.add(HitTestEntry::new(RenderId::new(2)).scroll_target(scroll));
    });
    Rig {
        lane,
        binding: GestureBinding::new(),
        seen,
        scrolled,
        path,
    }
}

impl Rig {
    fn send(&self, event: &PointerEvent) {
        self.lane.enter(|| {
            self.binding
                .handle_pointer_event(event, |_| self.path.clone());
        });
    }
}

#[test]
fn a_touchpad_pan_drives_a_declared_feed() {
    let rig = rig(true);
    rig.send(&pan(ScrollPhase::Began, 0.0, 1));
    rig.send(&pan(ScrollPhase::Changed, 10.0, 2));
    rig.send(&pan(ScrollPhase::Changed, 5.0, 3));
    rig.send(&pan(ScrollPhase::Ended, 0.0, 4));
    // The fingers move opposite to the scroll offset's growth (10 then 5 px of
    // offset is 15 px up), and the pointer path coalesces the two moves into the
    // one it delivers before the Up, as it does for a touch.
    assert_eq!(
        *rig.seen.borrow(),
        vec![
            Seen::Down(Point::new(50.0, 60.0)),
            Seen::Move(Point::new(50.0, 45.0)),
            Seen::Up(Point::new(50.0, 45.0)),
        ]
    );
    assert!(
        rig.scrolled.borrow().is_empty(),
        "the scroll target never saw a pan its feed took"
    );
}

#[test]
fn an_undeclared_pointer_handler_leaves_the_pan_to_the_scroll_route() {
    let rig = rig(false);
    rig.send(&pan(ScrollPhase::Began, 0.0, 1));
    rig.send(&pan(ScrollPhase::Changed, 10.0, 2));
    rig.send(&pan(ScrollPhase::Ended, 0.0, 3));
    assert!(rig.seen.borrow().is_empty(), "a raw handler sees no sample");
    assert_eq!(*rig.scrolled.borrow(), vec![0.0, 10.0, 0.0]);
}

#[test]
fn notched_and_phase_less_scrolls_never_open_a_contact() {
    let rig = rig(true);
    rig.send(&packet(
        ScrollPhase::Began,
        ScrollPrecision::Notched,
        0.0,
        1,
    ));
    rig.send(&packet(
        ScrollPhase::Changed,
        ScrollPrecision::Notched,
        10.0,
        2,
    ));
    rig.send(&packet(
        ScrollPhase::Ended,
        ScrollPrecision::Notched,
        0.0,
        3,
    ));
    assert!(rig.seen.borrow().is_empty());
    assert_eq!(*rig.scrolled.borrow(), vec![0.0, 10.0, 0.0]);
}

#[test]
fn a_cancelled_pan_cancels_its_contact() {
    let rig = rig(true);
    rig.send(&pan(ScrollPhase::Began, 0.0, 1));
    rig.send(&pan(ScrollPhase::Changed, 10.0, 2));
    rig.send(&pan(ScrollPhase::Cancelled, 0.0, 3));
    assert_eq!(
        *rig.seen.borrow(),
        vec![
            Seen::Down(Point::new(50.0, 60.0)),
            Seen::Move(Point::new(50.0, 50.0)),
            Seen::Cancel,
        ]
    );
}

#[test]
fn a_second_began_cancels_the_contact_it_replaces() {
    let rig = rig(true);
    rig.send(&pan(ScrollPhase::Began, 0.0, 1));
    rig.send(&pan(ScrollPhase::Changed, 10.0, 2));
    rig.send(&pan(ScrollPhase::Began, 0.0, 3));
    assert_eq!(
        *rig.seen.borrow(),
        vec![
            Seen::Down(Point::new(50.0, 60.0)),
            Seen::Move(Point::new(50.0, 50.0)),
            Seen::Cancel,
            Seen::Down(Point::new(50.0, 60.0)),
        ]
    );
}

#[test]
fn momentum_after_a_fed_pan_reaches_nobody() {
    let rig = rig(true);
    rig.send(&pan(ScrollPhase::Began, 0.0, 1));
    rig.send(&pan(ScrollPhase::Changed, 10.0, 2));
    rig.send(&pan(ScrollPhase::Ended, 0.0, 3));
    rig.send(&pan(ScrollPhase::MomentumBegan, 4.0, 4));
    rig.send(&pan(ScrollPhase::MomentumChanged, 3.0, 5));
    rig.send(&pan(ScrollPhase::MomentumEnded, 0.0, 6));
    assert!(
        rig.scrolled.borrow().is_empty(),
        "the recognizer ended with its own velocity; the platform's momentum is not a second scroll"
    );
    // The drop ended with the momentum: a plain wheel tick scrolls again.
    rig.send(&packet(
        ScrollPhase::Changed,
        ScrollPrecision::Notched,
        7.0,
        7,
    ));
    assert_eq!(*rig.scrolled.borrow(), vec![7.0]);
}

mod identity {
    use flui_platform::pointer::{
        PointerButton, PointerButtons, PointerPress, PointerRelease, PointerSample,
    };

    use super::*;

    /// What the feed saw of a contact, by pointer id.
    #[derive(Debug, Clone, PartialEq)]
    enum Logged {
        Down(u64),
        Up(u64),
        Cancel(u64),
    }

    struct Setup {
        lane: InteractionLane,
        binding: Rc<GestureBinding>,
        path: Rc<HitTestResult>,
        log: Rc<RefCell<Vec<Logged>>>,
    }

    /// A feed, and a raw handler that runs `on_scroll` for each scroll packet.
    fn setup(on_scroll: impl Fn(&GestureBinding, &HitTestResult) + 'static) -> Setup {
        let lane = InteractionLane::try_new().expect("lane");
        let handle = lane.dispatch_handle();
        let binding = Rc::new(GestureBinding::new());
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut path = HitTestResult::new();
        let shared: Rc<RefCell<Option<Rc<HitTestResult>>>> = Rc::new(RefCell::new(None));
        lane.enter(|| {
            let seen = Rc::clone(&log);
            let feed: PointerTarget = handle
                .register_pointer(move |dispatch| match dispatch.global {
                    PointerEvent::Down(down) => seen
                        .borrow_mut()
                        .push(Logged::Down(down.pointer.id.get().get())),
                    PointerEvent::Up(up) => seen
                        .borrow_mut()
                        .push(Logged::Up(up.pointer.id.get().get())),
                    PointerEvent::Cancel(cancel) => {
                        seen.borrow_mut()
                            .push(Logged::Cancel(cancel.pointer.id.get().get()));
                    }
                    _ => {}
                })
                .expect("feed");
            handle
                .declare_scroll_contact_feed(feed)
                .expect("declaration");
            let binding = Rc::downgrade(&binding);
            let route = Rc::clone(&shared);
            let raw: PointerTarget = handle
                .register_pointer(move |dispatch| {
                    if matches!(dispatch.global, PointerEvent::Scroll(_))
                        && let (Some(binding), Some(path)) =
                            (binding.upgrade(), route.borrow().clone())
                    {
                        on_scroll(&binding, &path);
                    }
                })
                .expect("raw handler");
            path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(feed));
            path.add(HitTestEntry::new(RenderId::new(2)).pointer_target(raw));
        });
        let path = Rc::new(path);
        *shared.borrow_mut() = Some(Rc::clone(&path));
        Setup {
            lane,
            binding,
            path,
            log,
        }
    }

    impl Setup {
        fn send(&self, event: &PointerEvent) {
            self.lane.enter(|| {
                self.binding
                    .handle_pointer_event(event, |_| (*self.path).clone());
                self.binding.flush_pending_moves();
            });
        }
    }

    fn native(id: u64, kind: PointerKind) -> PointerInfo {
        PointerInfo::new(PointerId::try_from(id).expect("pointer"), kind)
            .with_device(DeviceId::try_from(2_u64).expect("device"))
    }

    fn sample(x: f64) -> PointerSample {
        PointerSample::new(
            EventTime::from_nanos(1),
            PointerPosition::try_new(Point::new(x, 5.0)).expect("position"),
        )
    }

    #[test]
    fn a_virtual_contact_steps_past_a_live_native_contact_with_its_id() {
        let setup = setup(|_, _| {});
        // The first pan's latch is sequence 1 and its contact's Down takes 2,
        // so the next latch is sequence 4 after one native Down.
        setup.send(&pan(ScrollPhase::Began, 0.0, 1));
        setup.send(&pan(ScrollPhase::Ended, 0.0, 2));
        let held = (1_u64 << 63) | 4;
        setup.send(&PointerEvent::Down(PointerPress::new(
            native(held, PointerKind::Touch),
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample(10.0),
        )));
        setup.log.borrow_mut().clear();
        setup.send(&pan(ScrollPhase::Began, 0.0, 3));
        setup.send(&pan(ScrollPhase::Ended, 0.0, 4));
        let log = setup.log.borrow();
        assert!(
            !log.iter()
                .any(|entry| matches!(entry, Logged::Cancel(id) if *id == held)),
            "the native contact was cancelled: {log:?}"
        );
        assert!(
            log.iter()
                .any(|entry| matches!(entry, Logged::Down(id) if *id != held)),
            "the pan opened its contact under another id: {log:?}"
        );
        drop(log);
        setup.send(&PointerEvent::Up(PointerRelease::new(
            native(held, PointerKind::Touch),
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample(10.0),
        )));
    }

    #[test]
    fn a_sequence_begun_by_a_handler_leaves_no_stale_contact() {
        let fired = std::cell::Cell::new(false);
        let setup = setup(move |binding, path| {
            if !fired.replace(true) {
                binding.handle_pointer_event(&pan(ScrollPhase::Began, 0.0, 2), |_| path.clone());
            }
        });
        setup.send(&pan(ScrollPhase::Began, 0.0, 1));
        setup.send(&pan(ScrollPhase::Changed, 10.0, 3));
        setup.send(&pan(ScrollPhase::Ended, 0.0, 4));
        let log = setup.log.borrow();
        let downs = log.iter().filter(|e| matches!(e, Logged::Down(_))).count();
        let ends = log
            .iter()
            .filter(|e| matches!(e, Logged::Up(_) | Logged::Cancel(_)))
            .count();
        assert_eq!(downs, ends, "every contact that opened ended: {log:?}");
    }
}

/// A feed backed by a horizontal drag recognizer and a vertical scroll target
/// beneath it, both declared: the two compete for the pan (ADR-0251 §2).
struct Contest {
    lane: InteractionLane,
    binding: GestureBinding,
    started: Rc<RefCell<u32>>,
    scrolled: Rc<RefCell<Vec<(f64, f64)>>>,
    path: HitTestResult,
}

fn contest(scroll_innermost: bool, can_scroll: bool) -> Contest {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let started = Rc::new(RefCell::new(0));
    let scrolled = Rc::new(RefCell::new(Vec::new()));
    let mut path = HitTestResult::new();
    lane.enter(|| {
        let count = Rc::clone(&started);
        let recognizer =
            DragGestureRecognizer::builder(binding.arena().clone(), DragAxis::Horizontal)
                .on_start(move |_| *count.borrow_mut() += 1)
                .build();
        let feed: PointerTarget = handle
            .register_pointer(move |dispatch| {
                if matches!(dispatch.local, PointerEvent::Down(_)) {
                    recognizer.add_pointer(dispatch);
                } else {
                    recognizer.handle_event(dispatch);
                }
            })
            .expect("pointer handler");
        handle
            .declare_scroll_contact_feed(feed)
            .expect("declaration");
        let log = Rc::clone(&scrolled);
        let scroll: ScrollTarget = handle
            .register_scroll(move |event| {
                log.borrow_mut().push((event.delta.x(), event.delta.y()));
                EventPropagation::Stop
            })
            .expect("scroll target");
        handle
            .declare_scroll_target_axis(scroll, Axis::Vertical, move || can_scroll)
            .expect("axis");
        let feed_entry = HitTestEntry::new(RenderId::new(1)).pointer_target(feed);
        let scroll_entry = HitTestEntry::new(RenderId::new(2)).scroll_target(scroll);
        if scroll_innermost {
            path.add(scroll_entry);
            path.add(feed_entry);
        } else {
            path.add(feed_entry);
            path.add(scroll_entry);
        }
    });
    Contest {
        lane,
        binding,
        started,
        scrolled,
        path,
    }
}

impl Contest {
    fn send(&self, phase: ScrollPhase, dx: f64, dy: f64, nanos: u64) {
        self.send_within_frame(phase, dx, dy, nanos);
        self.lane.enter(|| {
            self.binding.flush_pending_moves();
        });
    }

    /// A packet whose move has not yet had its frame.
    fn send_within_frame(&self, phase: ScrollPhase, dx: f64, dy: f64, nanos: u64) {
        let event = PointerEvent::Scroll(
            ScrollEvent::new(
                source(),
                EventTime::from_nanos(nanos),
                PointerPosition::try_new(Point::new(50.0, 60.0)).expect("position"),
                ScrollDelta::try_new(ScrollUnit::Pixels, dx, dy).expect("delta"),
            )
            .with_precision(ScrollPrecision::Precise)
            .with_phase(phase),
        );
        self.lane.enter(|| {
            self.binding
                .handle_pointer_event(&event, |_| self.path.clone());
        });
    }
}

#[test]
fn a_pan_along_the_scroll_axis_is_replayed_to_the_scroll_target() {
    let contest = contest(false, true);
    contest.send(ScrollPhase::Began, 0.0, 0.0, 1);
    contest.send(ScrollPhase::Changed, 0.0, 4.0, 2);
    assert!(
        contest.scrolled.borrow().is_empty(),
        "the wait for the verdict holds the packets back"
    );
    contest.send(ScrollPhase::Changed, 0.0, 80.0, 3);
    contest.send(ScrollPhase::Changed, 0.0, 5.0, 4);
    assert_eq!(
        *contest.scrolled.borrow(),
        vec![(0.0, 0.0), (0.0, 4.0), (0.0, 80.0), (0.0, 5.0)],
        "the buffered packets arrive first, in order, then the live ones"
    );
    assert_eq!(*contest.started.borrow(), 0, "the horizontal feed lost");
}

#[test]
fn a_pan_along_the_feed_axis_goes_to_the_feed() {
    let contest = contest(false, true);
    contest.send(ScrollPhase::Began, 0.0, 0.0, 1);
    contest.send(ScrollPhase::Changed, 80.0, 0.0, 2);
    contest.send(ScrollPhase::Changed, 5.0, 0.0, 3);
    contest.send(ScrollPhase::Ended, 0.0, 0.0, 4);
    contest.send(ScrollPhase::MomentumBegan, 3.0, 0.0, 5);
    assert_eq!(*contest.started.borrow(), 1);
    assert!(
        contest.scrolled.borrow().is_empty(),
        "the scroll target saw nothing of a pan its competitor won, momentum included"
    );
}

#[test]
fn a_release_before_slop_leaves_the_pan_to_the_scroll_target() {
    for scroll_innermost in [true, false] {
        let contest = contest(scroll_innermost, true);
        contest.send(ScrollPhase::Began, 0.0, 0.0, 1);
        contest.send(ScrollPhase::Changed, 0.0, 1.0, 2);
        contest.send(ScrollPhase::Ended, 0.0, 0.0, 3);
        assert_eq!(*contest.started.borrow(), 0);
        assert_eq!(
            *contest.scrolled.borrow(),
            vec![(0.0, 0.0), (0.0, 1.0), (0.0, 0.0)],
            "scroll target innermost: {scroll_innermost}"
        );
    }
}

#[test]
fn a_release_settles_a_move_that_had_no_frame_yet() {
    let contest = contest(false, true);
    contest.send(ScrollPhase::Began, 0.0, 0.0, 1);
    contest.send_within_frame(ScrollPhase::Changed, 80.0, 0.0, 2);
    contest.send_within_frame(ScrollPhase::Ended, 0.0, 0.0, 3);
    assert_eq!(*contest.started.borrow(), 1, "the feed crossed slop");
    assert!(contest.scrolled.borrow().is_empty());
}

#[test]
fn a_scroll_target_with_nowhere_to_go_sits_the_contest_out() {
    let contest = contest(true, false);
    contest.send(ScrollPhase::Began, 0.0, 0.0, 1);
    contest.send(ScrollPhase::Changed, 0.0, 80.0, 2);
    contest.send(ScrollPhase::Changed, 0.0, 5.0, 3);
    assert!(
        contest.scrolled.borrow().is_empty(),
        "an ineligible target neither competes nor keeps the pan"
    );
}
