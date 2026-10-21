//! A phased touchpad pan opens an arena contact for the pointer targets that
//! declared they feed a recognizer (ADR-0251).

use std::{cell::RefCell, rc::Rc};

use flui_foundation::geometry::Point;
use flui_interaction::{
    EventPropagation, GestureBinding, HitTestEntry, HitTestResult, InteractionLane, PointerTarget,
    RenderId, ScrollTarget,
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
