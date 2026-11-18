//! A touchpad swipe through the binding: every phase reaches the path its
//! start took, its arena contest is keyed by its pointer, and each way the
//! binding ends a swipe itself ends it once, as a cancel, without settling a
//! contest that is not the swipe's own.

use std::cell::RefCell;
use std::num::NonZeroU8;
use std::rc::{Rc, Weak};

use flui_foundation::geometry::Point;
use flui_interaction::__runtime::{CloseMode, close_gestures};
use flui_interaction::gesture::GestureContest;
use flui_interaction::{
    binding::GestureBinding,
    gesture::{
        ArenaMembership, CancelOutcome, GestureArenaEntry, GestureArenaMember, GestureRecognizer,
        RecognizerSet,
    },
    pointer::{
        HitTestEntry, HitTestResult, InteractionLane, PointerDispatch, PointerTarget, RenderId,
    },
};
use flui_platform::EventTime;
use flui_platform::pointer::{
    DeviceId, PointerDeviceChange, PointerEvent, PointerId, PointerKind, PointerPosition,
    PointerSource, TouchpadSwipeEvent, TouchpadSwipePhase,
};

type Log = Rc<RefCell<Vec<String>>>;

/// What a listener does when it hears a swipe's end.
type OnEnd = Box<dyn Fn(&Rc<GestureBinding>)>;

const DEVICE: u64 = 5;

fn pointer(id: u64) -> PointerSource {
    PointerSource::new(
        PointerId::try_from(id).expect("nonzero"),
        PointerKind::Trackpad,
    )
    .with_device(DeviceId::try_from(DEVICE).expect("nonzero"))
}

fn swipe(id: u64, phase: TouchpadSwipePhase) -> PointerEvent {
    PointerEvent::TouchpadSwipe(TouchpadSwipeEvent::new(
        pointer(id),
        EventTime::from_nanos(1),
        PointerPosition::try_new(Point::new(10.0, 10.0)).expect("finite"),
        NonZeroU8::new(3).expect("three fingers"),
        phase,
    ))
}

/// A recognizer that joins the contest of every swipe it is offered and
/// logs its verdict.
struct Joiner {
    membership: ArenaMembership,
    entry: RefCell<Option<GestureArenaEntry>>,
    log: Log,
}

impl GestureArenaMember for Joiner {
    fn accept_gesture(&self, _: GestureContest) {
        self.log.borrow_mut().push("won".to_owned());
    }

    fn reject_gesture(&self, _: GestureContest) {
        self.log.borrow_mut().push("lost".to_owned());
    }
}

impl GestureRecognizer for Joiner {
    fn add_pointer(&self, _: PointerDispatch<'_>) {}

    fn add_touchpad_swipe(&self, start: PointerDispatch<'_>) {
        let PointerEvent::TouchpadSwipe(swipe) = start.local else {
            return;
        };
        *self.entry.borrow_mut() = self.membership.join(swipe.pointer().id);
    }

    fn handle_event(&self, _: PointerDispatch<'_>) {}

    fn cancel(&self) -> CancelOutcome {
        CancelOutcome::Idle
    }
}

/// A binding over one target that logs the phases it hears (and panics on
/// `Update` when `panics`), with a joiner attached, and the hit path.
struct Fixture {
    lane: InteractionLane,
    binding: Rc<GestureBinding>,
    log: Log,
    target: Option<PointerTarget>,
    _joiner: Rc<Joiner>,
}

impl Fixture {
    fn new(panics: bool, on_end: Option<OnEnd>) -> Self {
        let lane = InteractionLane::try_new().expect("lane");
        let binding = Rc::new(GestureBinding::new());
        let log = Log::default();
        let joiner = Rc::new_cyclic(|this: &Weak<Joiner>| Joiner {
            membership: ArenaMembership::new(binding.arena().clone(), this.clone()),
            entry: RefCell::new(None),
            log: Rc::clone(&log),
        });
        let mut set = RecognizerSet::default();
        set.attach(&joiner);
        let heard = Rc::clone(&log);
        let owner = Rc::downgrade(&binding);
        let target = lane.enter(|| {
            lane.dispatch_handle()
                .register_pointer(move |dispatch| {
                    if let PointerEvent::TouchpadSwipe(swipe) = dispatch.local {
                        heard.borrow_mut().push(format!(
                            "{} {}",
                            swipe.pointer().id.get(),
                            phase_name(swipe.phase)
                        ));
                        assert!(
                            !(panics && matches!(swipe.phase, TouchpadSwipePhase::Update(_))),
                            "a listener fails mid-swipe"
                        );
                        if swipe.phase == TouchpadSwipePhase::End
                            && let (Some(on_end), Some(binding)) = (&on_end, owner.upgrade())
                        {
                            on_end(&binding);
                        }
                    }
                    set.dispatch(dispatch);
                })
                .expect("target")
        });
        Self {
            lane,
            binding,
            log,
            target: Some(target),
            _joiner: joiner,
        }
    }

    fn send(&self, event: &PointerEvent) {
        let target = self.target.expect("registered");
        self.lane.enter(|| {
            self.binding.handle_pointer_event(event, |_| {
                let mut path = HitTestResult::new();
                path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                path
            });
        });
    }

    fn lines(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

fn phase_name(phase: TouchpadSwipePhase) -> &'static str {
    match phase {
        TouchpadSwipePhase::Start => "Start",
        TouchpadSwipePhase::Update(_) => "Update",
        TouchpadSwipePhase::End => "End",
        TouchpadSwipePhase::Cancelled => "Cancelled",
    }
}

fn start(id: u64) -> PointerEvent {
    swipe(id, TouchpadSwipePhase::Start)
}

/// Each way a swipe ends: its path hears one `End` or `Cancelled`, its
/// contest is swept or abandoned once, and a later phase reaches no one but a
/// newer swipe.
#[test]
fn every_way_a_swipe_ends_ends_it_once() {
    let late_update = || {
        swipe(
            1,
            TouchpadSwipePhase::Update(
                flui_platform::pointer::SwipeOffset::try_new(
                    flui_foundation::geometry::Offset::new(5.0, 0.0),
                )
                .expect("finite"),
            ),
        )
    };
    type Case = (&'static str, fn(&Fixture), &'static [&'static str]);
    let cases: &[Case] = &[
        (
            "the platform ends it: the lone member wins the sweep",
            |fixture| fixture.send(&swipe(1, TouchpadSwipePhase::End)),
            &["1 Start", "1 End", "won"],
        ),
        (
            "the platform cancels it: the contest is abandoned",
            |fixture| fixture.send(&swipe(1, TouchpadSwipePhase::Cancelled)),
            &["1 Start", "1 Cancelled", "lost"],
        ),
        (
            "the window loses the focus",
            |fixture| {
                fixture
                    .lane
                    .enter(|| fixture.binding.cancel_active_pointers());
            },
            &["1 Start", "1 Cancelled", "lost"],
        ),
        (
            "its device is removed",
            |fixture| {
                fixture.send(&PointerEvent::DeviceRemoved(PointerDeviceChange::new(
                    DeviceId::try_from(DEVICE).expect("nonzero"),
                    PointerKind::Trackpad,
                    EventTime::from_nanos(2),
                )));
            },
            &["1 Start", "1 Cancelled", "lost"],
        ),
        (
            "a newer start of its pointer displaces it",
            |fixture| fixture.send(&start(1)),
            // The newer swipe is the open one: the later step is its.
            &["1 Start", "1 Cancelled", "lost", "1 Start", "1 Update"],
        ),
        (
            "the binding closes",
            |fixture| {
                fixture
                    .lane
                    .enter(|| close_gestures(&fixture.binding, CloseMode::Ordinary));
            },
            &["1 Start", "lost"],
        ),
    ];
    for (name, end, expected) in cases {
        let fixture = Fixture::new(false, None);
        fixture.send(&start(1));
        end(&fixture);
        fixture.send(&late_update());
        assert_eq!(fixture.lines(), *expected, "{name}");
    }
}

/// A listener that fails mid-swipe has its panic resumed, and the swipe goes
/// on: its end still reaches the path and settles its contest.
#[test]
fn a_listener_that_fails_mid_swipe_leaves_the_swipe_open() {
    let fixture = Fixture::new(true, None);
    fixture.send(&start(1));
    let update = swipe(
        1,
        TouchpadSwipePhase::Update(
            flui_platform::pointer::SwipeOffset::try_new(flui_foundation::geometry::Offset::new(
                5.0, 0.0,
            ))
            .expect("finite"),
        ),
    );
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fixture.send(&update)));
    assert!(failed.is_err(), "the listener's panic is resumed");
    fixture.send(&swipe(1, TouchpadSwipePhase::End));
    assert_eq!(fixture.lines(), ["1 Start", "1 Update", "1 End", "won"]);
}

/// A swipe's end settles its own contest: a newer start of the same pointer
/// that a callback sends while the end is delivered opens a contest of its
/// own, which the end does not sweep and which waits for its own end.
#[test]
fn an_end_settles_its_own_contest_not_a_newer_one() {
    let path: Rc<RefCell<Option<PointerTarget>>> = Rc::default();
    let newer = Rc::clone(&path);
    let fixture = Fixture::new(
        false,
        Some(Box::new(move |binding: &Rc<GestureBinding>| {
            let target = newer.borrow().expect("registered");
            binding.handle_pointer_event(&start(1), |_| {
                let mut path = HitTestResult::new();
                path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
                path
            });
        })),
    );
    *path.borrow_mut() = fixture.target;
    fixture.send(&start(1));
    fixture.send(&swipe(1, TouchpadSwipePhase::End));
    let id = PointerId::try_from(1_u64).expect("nonzero");
    assert_eq!(
        fixture.lines(),
        ["1 Start", "1 End", "1 Start", "won"],
        "the old contest is swept alone"
    );
    assert_eq!(
        fixture.binding.arena().member_count(id),
        1,
        "the newer swipe's contest is still open with its member"
    );
    fixture.send(&swipe(1, TouchpadSwipePhase::End));
    assert_eq!(fixture.lines().last().map(String::as_str), Some("won"));
}
